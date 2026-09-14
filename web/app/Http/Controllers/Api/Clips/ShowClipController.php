<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Clips;

use App\Http\Resources\Api\ClipResource;
use App\Models\Clip;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;

final class ShowClipController
{
    public function __invoke(string $clip, #[CurrentUser] User $user): ClipResource
    {
        return new ClipResource(Clip::findFor($user, $clip));
    }
}
