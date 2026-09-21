<?php

declare(strict_types=1);

namespace App\Events;

use Illuminate\Foundation\Events\Dispatchable;

final readonly class DirectMessageDeleted implements SfuEvent
{
    use Dispatchable;

    public function __construct(public int $id, public int $senderId, public int $recipientId) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['user.'.$this->senderId, 'user.'.$this->recipientId];
    }

    public function eventName(): string
    {
        return 'DirectMessageDeleted';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['id' => $this->id];
    }
}
