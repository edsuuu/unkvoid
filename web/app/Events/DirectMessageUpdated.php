<?php

declare(strict_types=1);

namespace App\Events;

use App\Http\Resources\Api\DirectMessageResource;
use App\Models\DirectMessage;
use Illuminate\Foundation\Events\Dispatchable;

final readonly class DirectMessageUpdated implements SfuEvent
{
    use Dispatchable;

    public function __construct(public DirectMessage $message) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['user.'.$this->message->sender_id, 'user.'.$this->message->recipient_id];
    }

    public function eventName(): string
    {
        return 'DirectMessageUpdated';
    }

    /**
     * Sem `mine`, pelo mesmo motivo do `DirectMessageCreated`: um pacote, dois donos.
     *
     * @return array<string, mixed>
     */
    public function payload(): array
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
