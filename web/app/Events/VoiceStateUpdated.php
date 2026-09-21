<?php

declare(strict_types=1);

namespace App\Events;

use Illuminate\Foundation\Events\Dispatchable;

final readonly class VoiceStateUpdated implements SfuEvent
{
    use Dispatchable;

    public function __construct(
        public string $channelId,
        public int $userId,
        public string $name,
        public string $event,
    ) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['channel.'.$this->channelId];
    }

    public function eventName(): string
    {
        return 'VoiceStateUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['channel_id' => $this->channelId, 'user_id' => $this->userId, 'name' => $this->name, 'event' => $this->event];
    }
}
