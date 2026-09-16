<?php

declare(strict_types=1);

use App\Enums\PermissionEnum;
use App\Events\MessageDeleted;
use App\Events\MessageSent;
use App\Events\MessageUpdated;
use App\Models\Channel;
use App\Models\Message;
use App\Models\Server;
use App\Models\User;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;
use OwenIt\Auditing\Models\Audit;

beforeEach(function (): void {
    Http::fake();
});

it('manda, edita e apaga mensagem com as regras do Discord', function (): void {
    Event::fake([MessageSent::class, MessageUpdated::class, MessageDeleted::class]);

    $owner = User::factory()->create();
    $author = User::factory()->create();
    $other = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    joinServer($server, $other);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $messageId = $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi'])
        ->assertCreated()
        ->assertJsonPath('data.body', 'oi')
        ->assertJsonPath('data.user.id', $author->id)
        ->assertJsonPath('data.channel_id', $channel->id)
        ->json('data.id');

    Event::assertDispatched(MessageSent::class, fn (MessageSent $event): bool => $event->message->id === $messageId && $event->broadcastOn()->name === "private-channel.{$channel->id}");

    $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => ''])->assertUnprocessable();
    $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => str_repeat('a', 2001)])->assertUnprocessable();

    $this->actingAs($other, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertForbidden();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertForbidden();
    $this->actingAs($author, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertOk()->assertJsonPath('data.body', 'editado');

    Event::assertDispatched(MessageUpdated::class);

    $this->actingAs($other, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    Event::assertDispatched(MessageDeleted::class, fn (MessageDeleted $event): bool => $event->id === $messageId && $event->channelId === $channel->id);

    // Apagar virou soft delete: a mensagem sai da conversa e continua no banco, que é de
    // onde a auditoria lê o que foi dito.
    $this->assertSoftDeleted('messages', ['id' => $messageId]);
    expect(Message::query()->count())->toBe(0);
});

it('quem tem MANAGE_MESSAGES apaga a mensagem dos outros, quem não tem SEND_MESSAGES não escreve', function (): void {
    $owner = User::factory()->create();
    $author = User::factory()->create();
    $moderator = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    joinServer($server, $moderator);
    giveRole($server, $moderator, PermissionEnum::ManageMessages->value);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $messageId = $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi'])->assertCreated()->json('data.id');

    $this->actingAs($moderator, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    $channel->overwrites()->create(['target_type' => 'member', 'target_id' => $author->id, 'allow' => 0, 'deny' => PermissionEnum::SendMessages->value]);

    $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'de novo'])->assertForbidden();
});

it('a auditoria registra quem editou e quem apagou a mensagem, e guarda o corpo anterior', function (): void {
    config(['audit.console' => true]);
    $owner = User::factory()->create();
    $author = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $messageId = $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi'])->assertCreated()->json('data.id');

    $this->actingAs($author, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertOk();

    $edited = Audit::query()->where('event', 'updated')->where('auditable_type', Message::class)->where('auditable_id', $messageId)->firstOrFail();

    expect($edited->user_id)->toBe($author->id)
        ->and($edited->old_values['body'])->toBe('oi')
        ->and($edited->new_values['body'])->toBe('editado');

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    $deleted = Audit::query()->where('event', 'deleted')->where('auditable_type', Message::class)->where('auditable_id', $messageId)->firstOrFail();

    expect($deleted->user_id)->toBe($owner->id)
        ->and($deleted->old_values['body'])->toBe('editado');
});

it('lista as 50 mais recentes antes de um id, em ordem crescente', function (): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = Channel::query()->where('server_id', $server->id)->where('type', 'text')->firstOrFail();

    for ($index = 1; $index <= 60; $index++) {
        $channel->messages()->create(['user_id' => $owner->id, 'body' => "mensagem {$index}"]);
    }

    $page = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');

    expect($page)->toHaveCount(50)
        ->and($page[0]['body'])->toBe('mensagem 11')
        ->and($page[49]['body'])->toBe('mensagem 60');

    $older = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages?before={$page[0]['id']}")->assertOk()->json('data');

    expect($older)->toHaveCount(10)
        ->and($older[0]['body'])->toBe('mensagem 1');

    $this->actingAs(User::factory()->create(), 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertForbidden();
});

it('responder guarda a mensagem original, e so aceita mensagem do mesmo canal', function (): void {
    Event::fake([MessageSent::class]);

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();
    $other = Channel::query()->create(['server_id' => $server->id, 'name' => 'outro', 'type' => 'text', 'position' => 9]);

    $original = $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'quem vem hoje?'])
        ->assertCreated()
        ->json('data.id');

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'eu vou', 'reply_to_id' => $original])
        ->assertCreated()
        ->assertJsonPath('data.reply_to.id', $original)
        ->assertJsonPath('data.reply_to.name', $owner->name)
        ->assertJsonPath('data.reply_to.body', 'quem vem hoje?');

    // Responder mensagem de outro canal vazaria o texto de um canal que a pessoa pode nem ver.
    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$other->id}/messages", ['body' => 'de outro canal', 'reply_to_id' => $original])
        ->assertStatus(422);

    $history = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');

    expect(collect($history)->firstWhere('body', 'eu vou')['reply_to']['body'])->toBe('quem vem hoje?');

    // Apagar a original não leva a resposta junto: o fio continua de pé, só a citação some.
    // Apagar aqui é soft delete, então a coluna segue apontando e é a relação que não resolve.
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$original}")->assertNoContent();

    $depois = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');
    $resposta = collect($depois)->firstWhere('body', 'eu vou');

    expect($resposta)->not->toBeNull()
        ->and($resposta['reply_to'])->toBeNull();
});
