<?php

declare(strict_types=1);

namespace App\Events;

use Illuminate\Foundation\Events\Dispatchable;

final readonly class MemberRemoved implements SfuEvent
{
    use Dispatchable;

    public function __construct(public int $serverId, public int $userId, public string $reason) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['user.'.$this->userId];
    }

    public function eventName(): string
    {
        return 'MemberRemoved';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['server_id' => $this->serverId, 'reason' => $this->reason];
    }
}
