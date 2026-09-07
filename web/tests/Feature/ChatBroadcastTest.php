<?php

declare(strict_types=1);

use App\Actions\Servers\CreateServer;
use App\Events\MessageSent;
use App\Livewire\Workspace\Shell;
use App\Models\Server;
use App\Models\User;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Str;

use function Pest\Laravel\actingAs;

beforeEach(function (): void {
    $this->author = User::factory()->create(['nickname' => 'Autora']);
    $this->server = app(CreateServer::class)->handle($this->author, 'Sala');
    $this->channel = $this->server->channels()->where('type', 'text')->firstOrFail();
});

it('announces the message so the others receive it without reloading', function (): void {
    Event::fake([MessageSent::class]);

    actingAs($this->author);

    // serverId too: the channel is read from the open server's list, not straight
    // from the database, which is what keeps someone from writing into a server they
    // are not in by setting an id by hand.
    Livewire::test(Shell::class)
        ->set('serverId', $this->server->id)
        ->set('channelId', $this->channel->id)
        ->set('draft', 'oi')
        ->call('sendMessage');

    Event::assertDispatched(
        MessageSent::class,
        fn (MessageSent $event): bool => $event->message->content === 'oi'
            && $event->message->channel_id === $this->channel->id,
    );
});

/**
 * Queued, the broadcast waits for a worker that does not exist on this VPS — the message
 * is saved and simply never arrives. This is the whole reason there is no queue daemon.
 */
it('broadcasts without going through the queue', function (): void {
    expect(new MessageSent(new App\Models\Message()))
        ->toBeInstanceOf(Illuminate\Contracts\Broadcasting\ShouldBroadcastNow::class);
});

it('carries only the id, never the content', function (): void {
    $message = $this->channel->messages()->create([
        'user_id' => $this->author->id,
        'content' => 'segredo',
    ]);

    $payload = (new MessageSent($message))->broadcastWith();

    expect($payload)->toBe(['messageId' => $message->id, 'authorId' => $this->author->id])
        ->and(json_encode($payload))->not->toContain('segredo');
});

it('broadcasts on the private channel of that text channel only', function (): void {
    $message = $this->channel->messages()->create([
        'user_id' => $this->author->id,
        'content' => 'oi',
    ]);

    $channels = array_map('strval', (new MessageSent($message))->broadcastOn());

    expect($channels)->toBe(["private-channel.{$this->channel->id}"]);
});

/**
 * Through the real endpoint, not the closure: the socket must not become a side door
 * into a server someone was removed from, and this is the only place that is enforced
 * for listeners.
 */
it('lets a member listen and refuses everyone else', function (): void {
    $member = User::factory()->create();
    $stranger = User::factory()->create();

    $this->server->members()->create(['user_id' => $member->id, 'role' => 'member']);

    $authorize = fn (User $user) => actingAs($user)->post('/broadcasting/auth', [
        'socket_id' => '1234.5678',
        'channel_name' => "private-channel.{$this->channel->id}",
    ]);

    expect($authorize($this->author)->status())->toBe(200)
        ->and($authorize($member)->status())->toBe(200)
        ->and($authorize($stranger)->status())->toBe(403);
});

it('refuses a channel that does not exist', function (): void {
    actingAs($this->author)
        ->post('/broadcasting/auth', [
            'socket_id' => '1234.5678',
            'channel_name' => 'private-channel.'.Str::uuid()->toString(),
        ])
        ->assertForbidden();
});
