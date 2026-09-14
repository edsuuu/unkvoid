<?php

declare(strict_types=1);

namespace App\Events;

use App\Http\Resources\Api\MessageResource;
use App\Models\Message;
use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;

final class MessageSent implements ShouldBroadcastNow
{
    use Dispatchable;
    use InteractsWithSockets;

    public function __construct(public readonly Message $message) {}

    public function broadcastOn(): PrivateChannel
    {
        return new PrivateChannel('channel.'.$this->message->channel_id);
    }

    public function broadcastAs(): string
    {
        return 'MessageSent';
    }

    /**
     * @return array<string, mixed>
     */
    public function broadcastWith(): array
    {
        return ['message' => new MessageResource($this->message)->resolve()];
    }
}
