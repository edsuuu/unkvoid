<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Resources\Api\ConfigResource;

final class ConfigController
{
    public function __invoke(): ConfigResource
    {
        return new ConfigResource;
    }
}
