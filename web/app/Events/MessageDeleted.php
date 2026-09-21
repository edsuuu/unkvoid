<?php

declare(strict_types=1);

namespace App\Events;

use Illuminate\Foundation\Events\Dispatchable;

final readonly class MessageDeleted implements SfuEvent
{
    use Dispatchable;

    public function __construct(public int $id, public string $channelId) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['channel.'.$this->channelId];
    }

    public function eventName(): string
    {
        return 'MessageDeleted';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['id' => $this->id, 'channel_id' => $this->channelId];
    }
}
