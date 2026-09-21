<?php

declare(strict_types=1);

namespace App\Events;

/**
 * O tempo real sai pelo SFU: o evento só diz para quais canais vai, com que nome chega no
 * app e o que leva dentro. Quem publica é o `LogsFailedWrites::publish()`.
 */
interface SfuEvent
{
    /**
     * @return array<int, string>
     */
    public function channels(): array;

    public function eventName(): string;

    /**
     * @return array<string, mixed>
     */
    public function payload(): array;
}
