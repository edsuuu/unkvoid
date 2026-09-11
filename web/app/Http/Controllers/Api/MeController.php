<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Resources\Api\UserResource;
use Illuminate\Http\Request;

final class MeController
{
    public function __invoke(Request $request): UserResource
    {
        return new UserResource($request->user());
    }
}
