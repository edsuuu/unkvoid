<?php

declare(strict_types=1);

namespace App\Events;

use Illuminate\Foundation\Events\Dispatchable;

/**
 * Qualquer mudança de estrutura: o app refaz o GET do servidor.
 */
final readonly class ServerUpdated implements SfuEvent
{
    use Dispatchable;

    public function __construct(public int $serverId) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['server.'.$this->serverId];
    }

    public function eventName(): string
    {
        return 'ServerUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['server_id' => $this->serverId];
    }
}
