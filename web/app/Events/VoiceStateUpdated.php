<?php

declare(strict_types=1);

namespace App\Events;

use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;

final class VoiceStateUpdated implements ShouldBroadcastNow
{
    use Dispatchable;
    use InteractsWithSockets;

    public function __construct(
        public readonly string $channelId,
        public readonly int $userId,
        public readonly string $name,
        public readonly string $event,
    ) {}

    public function broadcastOn(): PrivateChannel
    {
        return new PrivateChannel('channel.'.$this->channelId);
    }

    public function broadcastAs(): string
    {
        return 'VoiceStateUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function broadcastWith(): array
    {
        return ['channel_id' => $this->channelId, 'user_id' => $this->userId, 'name' => $this->name, 'event' => $this->event];
    }
}
