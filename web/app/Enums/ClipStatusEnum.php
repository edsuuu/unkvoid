<?php

declare(strict_types=1);

namespace App\Enums;

enum ClipStatusEnum: string
{
    case Processing = 'processing';
    case Ready = 'ready';
    case Failed = 'failed';
}
