<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Clips;

use App\Http\Resources\Api\ClipResource;
use App\Models\Clip;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;

final class IndexClipController
{
    public function __invoke(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ClipResource::collection(Clip::listFor($user));
    }
}
