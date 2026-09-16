<?php

declare(strict_types=1);

namespace App\Events;

use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;

/**
 * Vai para os dois lados: quem pediu precisa ver "aceito" na hora, e quem recebeu precisa
 * ver o pedido chegar sem recarregar.
 */
final class FriendshipUpdated implements ShouldBroadcastNow
{
    use Dispatchable;
    use InteractsWithSockets;

    public function __construct(public readonly Friendship $friendship, public readonly bool $removed = false) {}

    /**
     * @return array<int, PrivateChannel>
     */
    public function broadcastOn(): array
    {
        return [
            new PrivateChannel('user.'.$this->friendship->requester_id),
            new PrivateChannel('user.'.$this->friendship->addressee_id),
        ];
    }

    public function broadcastAs(): string
    {
        return 'FriendshipUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function broadcastWith(): array
    {
        return [
            'friendship' => new FriendResource($this->friendship)->resolve(),
            'removed' => $this->removed,
        ];
    }
}
