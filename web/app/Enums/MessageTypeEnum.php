<?php

declare(strict_types=1);

namespace App\Enums;

/**
 * O que a mensagem é. `join` não tem corpo: o app escreve a frase a partir de quem entrou.
 */
enum MessageTypeEnum: string
{
    case User = 'user';
    case Join = 'join';
}
