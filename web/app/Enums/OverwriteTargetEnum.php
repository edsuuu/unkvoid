<?php

declare(strict_types=1);

namespace App\Enums;

enum OverwriteTargetEnum: string
{
    case Role = 'role';
    case Member = 'member';
}
