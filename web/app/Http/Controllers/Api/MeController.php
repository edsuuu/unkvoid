<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Auth\UpdateMeRequest;
use App\Http\Resources\Api\UserResource;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

/**
 * A conta de quem está logado.
 */
final class MeController
{
    public function show(#[CurrentUser] User $user): UserResource
    {
        return new UserResource($user);
    }

    /**
     * @throws Throwable
     */
    public function update(UpdateMeRequest $request, #[CurrentUser] User $user): UserResource
    {
        $user->confirmNickname($request->string('name')->toString());

        return new UserResource($user);
    }
}
