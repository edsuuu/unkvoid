<?php

declare(strict_types=1);

namespace App\Events;

use App\Models\Message;
use Illuminate\Broadcasting\Channel as BroadcastChannel;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;
use Illuminate\Queue\SerializesModels;
use Override;

/**
 * Announces a message to everyone in the channel.
 *
 * The payload carries only the id: whoever receives it asks the component to refresh,
 * and the component reads from the database with the permissions it already applies.
 * Broadcasting the content would mean a second place deciding who may read what.
 *
 * `ShouldBroadcastNow` and not `ShouldBroadcast`: queued, this would sit in the database
 * waiting for a worker, and a chat that arrives when a daemon feels like it is not a
 * chat. Sending is a local HTTP call to Reverb — cheaper than the daemon it saves.
 */
final class MessageSent implements ShouldBroadcastNow
{
    use Dispatchable;
    use SerializesModels;

    public function __construct(public readonly Message $message) {}

    /**
     * @return array<int, BroadcastChannel>
     */
    #[Override]
    public function broadcastOn(): array
    {
        return [new PrivateChannel("channel.{$this->message->channel_id}")];
    }

    /**
     * @return array<string, string>
     */
    public function broadcastWith(): array
    {
        return [
            'messageId' => $this->message->id,
            'authorId' => $this->message->user_id,
        ];
    }
}
