<?php

declare(strict_types=1);

use App\Enums\FriendshipStatusEnum;
use App\Events\DirectMessageCreated;
use App\Events\DirectMessageDeleted;
use App\Events\DirectMessageUpdated;
use App\Models\DirectMessage;
use App\Models\File;
use App\Models\Friendship;
use App\Models\Server;
use App\Models\User;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;

function befriend(User $one, User $other): Friendship
{
    $friendship = Friendship::request($one, $other);
    $friendship->accept($other);

    return $friendship->refresh();
}

it('conversa direta só entre amigos, com não lidas, edição e exclusão', function (): void {
    Event::fake([DirectMessageCreated::class, DirectMessageUpdated::class, DirectMessageDeleted::class]);

    $alice = User::factory()->create();
    $bob = User::factory()->create();

    $this->actingAs($alice, 'sanctum')->postJson("/api/dm/{$bob->id}", ['body' => 'oi'])->assertForbidden();
    $this->actingAs($alice, 'sanctum')->getJson("/api/dm/{$bob->id}")->assertForbidden();

    befriend($alice, $bob);

    $messageId = $this->actingAs($alice, 'sanctum')->postJson("/api/dm/{$bob->id}", ['body' => 'oi'])
        ->assertCreated()
        ->assertJsonPath('data.body', 'oi')
        ->assertJsonPath('data.mine', true)
        ->assertJsonPath('data.sender.id', $alice->id)
        ->json('data.id');

    Event::assertDispatched(DirectMessageCreated::class, fn (DirectMessageCreated $event): bool => $event->channels() === ["user.{$alice->id}", "user.{$bob->id}"]);

    $this->actingAs($alice, 'sanctum')->postJson("/api/dm/{$bob->id}", ['body' => ''])->assertUnprocessable();
    $this->actingAs($alice, 'sanctum')->postJson("/api/dm/{$bob->id}", ['body' => str_repeat('a', 2001)])->assertUnprocessable();

    $this->actingAs($bob, 'sanctum')->getJson('/api/dm')
        ->assertOk()
        ->assertJsonPath('data.0.user.id', $alice->id)
        ->assertJsonPath('data.0.last.body', 'oi')
        ->assertJsonPath('data.0.last.mine', false)
        ->assertJsonPath('data.0.unread', 1);

    $this->actingAs($bob, 'sanctum')->getJson("/api/dm/{$alice->id}")
        ->assertOk()
        ->assertJsonPath('data.0.body', 'oi')
        ->assertJsonPath('data.0.mine', false);

    // Abrir a conversa é o que marca como lida: o contador do app zera sozinho.
    $this->actingAs($bob, 'sanctum')->getJson('/api/dm')->assertJsonPath('data.0.unread', 0);

    $this->actingAs($bob, 'sanctum')->patchJson("/api/dm/{$messageId}", ['body' => 'editado'])->assertForbidden();
    $this->actingAs($alice, 'sanctum')->patchJson("/api/dm/{$messageId}", ['body' => 'editado'])
        ->assertOk()
        ->assertJsonPath('data.body', 'editado')
        ->assertJsonPath('data.edited_at', fn (?string $editedAt): bool => ! is_null($editedAt));

    Event::assertDispatched(DirectMessageUpdated::class);

    $this->actingAs($bob, 'sanctum')->deleteJson("/api/dm/{$messageId}")->assertForbidden();
    $this->actingAs($alice, 'sanctum')->deleteJson("/api/dm/{$messageId}")->assertNoContent();

    Event::assertDispatched(DirectMessageDeleted::class, fn (DirectMessageDeleted $event): bool => $event->id === $messageId);

    $this->assertSoftDeleted('direct_messages', ['id' => $messageId]);
    $this->actingAs($bob, 'sanctum')->getJson("/api/dm/{$alice->id}")->assertOk()->assertJsonCount(0, 'data');
});

it('bloquear fecha a conversa dos dois lados', function (): void {
    $alice = User::factory()->create();
    $bob = User::factory()->create();
    $friendship = befriend($alice, $bob);

    $this->actingAs($alice, 'sanctum')->postJson("/api/dm/{$bob->id}", ['body' => 'oi'])->assertCreated();

    $friendship->block($bob);

    expect($friendship->refresh()->status)->toBe(FriendshipStatusEnum::Blocked);

    $this->actingAs($alice, 'sanctum')->postJson("/api/dm/{$bob->id}", ['body' => 'de novo'])->assertForbidden();
    $this->actingAs($bob, 'sanctum')->postJson("/api/dm/{$alice->id}", ['body' => 'de novo'])->assertForbidden();
    $this->actingAs($alice, 'sanctum')->getJson("/api/dm/{$bob->id}")->assertForbidden();
});

it('lista as 50 mais recentes antes de um id, em ordem crescente, e uma linha por conversa', function (): void {
    $alice = User::factory()->create();
    $bob = User::factory()->create();
    $carol = User::factory()->create();
    befriend($alice, $bob);
    befriend($alice, $carol);

    for ($index = 1; $index <= 60; $index++) {
        DirectMessage::query()->create(['sender_id' => $alice->id, 'recipient_id' => $bob->id, 'body' => "mensagem {$index}"]);
    }

    DirectMessage::query()->create(['sender_id' => $carol->id, 'recipient_id' => $alice->id, 'body' => 'da carol']);

    $page = $this->actingAs($alice, 'sanctum')->getJson("/api/dm/{$bob->id}")->assertOk()->json('data');

    expect($page)->toHaveCount(50)
        ->and($page[0]['body'])->toBe('mensagem 11')
        ->and($page[49]['body'])->toBe('mensagem 60');

    $older = $this->actingAs($alice, 'sanctum')->getJson("/api/dm/{$bob->id}?before={$page[0]['id']}")->assertOk()->json('data');

    expect($older)->toHaveCount(10)
        ->and($older[0]['body'])->toBe('mensagem 1');

    $conversations = $this->actingAs($alice, 'sanctum')->getJson('/api/dm')->assertOk()->json('data');

    expect($conversations)->toHaveCount(2)
        ->and($conversations[0]['user']['id'])->toBe($carol->id)
        ->and($conversations[0]['unread'])->toBe(1)
        ->and($conversations[1]['user']['id'])->toBe($bob->id)
        ->and($conversations[1]['last']['body'])->toBe('mensagem 60');
});

it('o evento vai para os dois lados sem dizer de quem é a mensagem', function (): void {
    $alice = User::factory()->create();
    $bob = User::factory()->create();
    befriend($alice, $bob);

    $message = DirectMessage::send($alice, $bob, 'oi');
    $payload = new DirectMessageCreated($message)->payload();

    expect($payload['message'])->not->toHaveKey('mine')
        ->and($payload['message']['sender']['id'])->toBe($alice->id)
        ->and($payload['recipient']['id'])->toBe($bob->id)
        ->and(new DirectMessageDeleted($message->id, $alice->id, $bob->id)->payload())->toBe(['id' => $message->id]);
});

it('gente do mesmo servidor conversa sem ser amiga, e bloquear fecha mesmo assim', function (): void {
    Http::fake();

    $owner = User::factory()->create();
    $member = User::factory()->create();
    $stranger = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');

    joinServer($server, $member);

    // A ficha de perfil de um membro tem campo de mensagem: sem esta regra, mandar dali
    // daria 403 em todo mundo que ainda não é amigo — que é justamente quem se quer chamar.
    $this->actingAs($owner, 'sanctum')->postJson("/api/dm/{$member->id}", ['body' => 'bem-vindo'])
        ->assertCreated()
        ->assertJsonPath('data.body', 'bem-vindo');

    $this->actingAs($member, 'sanctum')->getJson("/api/dm/{$owner->id}")
        ->assertOk()
        ->assertJsonPath('data.0.body', 'bem-vindo');

    $this->actingAs($owner, 'sanctum')->postJson("/api/dm/{$stranger->id}", ['body' => 'oi'])
        ->assertForbidden();

    Friendship::request($owner, $member)->block($member);

    $this->actingAs($owner, 'sanctum')->postJson("/api/dm/{$member->id}", ['body' => 'de novo'])
        ->assertForbidden();
});

it('quem foi bloqueado nao desfaz o proprio bloqueio', function (): void {
    $alice = User::factory()->create();
    $bob = User::factory()->create();

    $friendship = befriend($alice, $bob);
    $friendship->block($alice);

    // As duas portas dos fundos: "aceitar" o próprio bloqueio e "recusar" a linha dele.
    $this->actingAs($bob, 'sanctum')->patchJson("/api/friends/{$friendship->id}", ['action' => 'accept'])->assertForbidden();
    $this->actingAs($bob, 'sanctum')->deleteJson("/api/friends/{$friendship->id}")->assertForbidden();

    expect($friendship->refresh()->status)->toBe(FriendshipStatusEnum::Blocked);

    $this->actingAs($bob, 'sanctum')->postJson("/api/dm/{$alice->id}", ['body' => 'voltei'])->assertForbidden();

    $this->actingAs($alice, 'sanctum')->deleteJson("/api/friends/{$friendship->id}")->assertNoContent();
});

it('marcar a conversa como lida zera o contador do outro lado', function (): void {
    $alice = User::factory()->create();
    $bob = User::factory()->create();

    befriend($alice, $bob);
    DirectMessage::send($bob, $alice, 'oi');

    expect($this->actingAs($alice, 'sanctum')->getJson('/api/dm')->assertOk()->json('data.0.unread'))->toBe(1);

    $this->actingAs($alice, 'sanctum')->postJson("/api/dm/{$bob->id}/read")->assertNoContent();

    expect($this->actingAs($alice, 'sanctum')->getJson('/api/dm')->assertOk()->json('data.0.unread'))->toBe(0);
});

it('mensagem direta não leva imagem: sem corpo continua 422, no envio e na edição', function (): void {
    Http::fake();
    $alice = User::factory()->create();
    $bob = User::factory()->create();
    befriend($alice, $bob);

    $this->actingAs($alice, 'sanctum')
        ->postJson("/api/dm/{$bob->id}", ['images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertUnprocessable()
        ->assertJsonValidationErrors('body');

    $messageId = $this->actingAs($alice, 'sanctum')
        ->postJson("/api/dm/{$bob->id}", ['body' => 'com foto', 'images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertCreated()
        ->assertJsonMissingPath('data.files')
        ->json('data.id');

    $this->actingAs($alice, 'sanctum')->patchJson("/api/dm/{$messageId}", ['body' => ''])->assertUnprocessable()->assertJsonValidationErrors('body');

    expect(File::query()->count())->toBe(0);
});
