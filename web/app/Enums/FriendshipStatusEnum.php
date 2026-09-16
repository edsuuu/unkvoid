<?php

declare(strict_types=1);

namespace App\Enums;

/**
 * Em que pé está o pedido. A linha é única por par, então bloquear não cria uma segunda:
 * é a mesma mudando de situação.
 */
enum FriendshipStatusEnum: string
{
    case Pending = 'pending';
    case Accepted = 'accepted';
    case Blocked = 'blocked';
}
