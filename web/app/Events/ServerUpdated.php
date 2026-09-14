<?php

declare(strict_types=1);

namespace App\Events;

use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PresenceChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;

/**
 * Qualquer mudança de estrutura: o app refaz o GET do servidor.
 */
final class ServerUpdated implements ShouldBroadcastNow
{
    use Dispatchable;
    use InteractsWithSockets;

    public function __construct(public readonly int $serverId) {}

    public function broadcastOn(): PresenceChannel
    {
        return new PresenceChannel('server.'.$this->serverId);
    }

    public function broadcastAs(): string
    {
        return 'ServerUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function broadcastWith(): array
    {
        return ['server_id' => $this->serverId];
    }
}
