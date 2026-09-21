<?php

declare(strict_types=1);

namespace App\Events;

use App\Http\Resources\Api\MessageResource;
use App\Models\Message;
use Illuminate\Foundation\Events\Dispatchable;

final readonly class MessageSent implements SfuEvent
{
    use Dispatchable;

    public function __construct(public Message $message) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['channel.'.$this->message->channel_id];
    }

    public function eventName(): string
    {
        return 'MessageSent';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['message' => new MessageResource($this->message)->resolve()];
    }
}
