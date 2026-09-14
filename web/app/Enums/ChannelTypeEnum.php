<?php

declare(strict_types=1);

namespace App\Enums;

enum ChannelTypeEnum: string
{
    case Text = 'text';
    case Voice = 'voice';
}
