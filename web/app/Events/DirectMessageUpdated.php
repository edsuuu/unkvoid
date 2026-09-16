<?php

declare(strict_types=1);

namespace App\Events;

use App\Http\Resources\Api\DirectMessageResource;
use App\Models\DirectMessage;
use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;

final class DirectMessageUpdated implements ShouldBroadcastNow
{
    use Dispatchable;
    use InteractsWithSockets;

    public function __construct(public readonly DirectMessage $message) {}

    /**
     * @return array<int, PrivateChannel>
     */
    public function broadcastOn(): array
    {
        return [
            new PrivateChannel('user.'.$this->message->sender_id),
            new PrivateChannel('user.'.$this->message->recipient_id),
        ];
    }

    public function broadcastAs(): string
    {
        return 'DirectMessageUpdated';
    }

    /**
     * Sem `mine`, pelo mesmo motivo do `DirectMessageCreated`: um pacote, dois donos.
     *
     * @return array<string, mixed>
     */
    public function broadcastWith(): array
    {
        $message = new DirectMessageResource($this->message)->resolve();
        unset($message['mine']);

        $recipient = $this->message->recipient;

        return [
            'message' => $message,
            'recipient' => ['id' => $recipient->id, 'name' => $recipient->name, 'avatar_url' => $recipient->avatar_url],
        ];
    }
}
