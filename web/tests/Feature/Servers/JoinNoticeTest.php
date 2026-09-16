<?php

declare(strict_types=1);

use App\Enums\MessageTypeEnum;
use App\Enums\PermissionEnum;
use App\Events\MessageSent;
use App\Models\Message;
use App\Models\Server;
use App\Models\User;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;

beforeEach(function (): void {
    Http::fake();
});

it('entrar por convite avisa no primeiro canal de texto, uma vez só', function (): void {
    Event::fake([MessageSent::class]);

    $owner = User::factory()->create();
    $guest = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertOk();

    $notice = Message::query()->where('channel_id', $channel->id)->firstOrFail();

    expect($notice->type)->toBe(MessageTypeEnum::Join)
        ->and($notice->user_id)->toBe($guest->id)
        // O corpo é para o app antigo, que não conhece `type`: sem ele, balão vazio.
        ->and($notice->body)->toBe('chegou no servidor!');

    Event::assertDispatched(MessageSent::class, fn (MessageSent $event): bool => $event->message->id === $notice->id && $event->broadcastOn()->name === "private-channel.{$channel->id}");

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertOk();

    expect(Message::query()->count())->toBe(1);

    $chat = $this->actingAs($guest, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');

    expect($chat)->toHaveCount(1)
        ->and($chat[0]['type'])->toBe('join')
        ->and($chat[0]['user']['id'])->toBe($guest->id);
});

it('o aviso pula o canal escondido e a mensagem de gente continua sendo user', function (): void {
    $owner = User::factory()->create();
    $guest = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $hidden = $server->channels()->where('type', 'text')->firstOrFail();
    $open = $server->channels()->create(['name' => 'avisos', 'type' => 'text', 'position' => 5]);

    $hidden->overwrites()->create([
        'target_type' => 'role',
        'target_id' => $server->everyoneRole()->id,
        'allow' => 0,
        'deny' => PermissionEnum::ViewChannel->value,
    ]);

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertOk();

    expect(Message::query()->where('channel_id', $hidden->id)->count())->toBe(0)
        ->and(Message::query()->where('channel_id', $open->id)->where('type', 'join')->count())->toBe(1);

    $this->actingAs($guest, 'sanctum')->postJson("/api/channels/{$open->id}/messages", ['body' => 'oi'])
        ->assertCreated()
        ->assertJsonPath('data.type', 'user');
});
