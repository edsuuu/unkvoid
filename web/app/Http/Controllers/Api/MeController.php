<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Auth\StoreAvatarRequest;
use App\Http\Requests\Api\Auth\UpdateMeRequest;
use App\Http\Resources\Api\UserResource;
use App\Models\User;
use App\Services\Storage\BucketService;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\UploadedFile;
use Throwable;

/**
 * A conta de quem está logado: o apelido e a foto de perfil.
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

    /**
     * @throws Throwable
     */
    public function storeAvatar(StoreAvatarRequest $request, #[CurrentUser] User $user, BucketService $bucket): UserResource
    {
        /** @var UploadedFile $avatar */
        $avatar = $request->file('avatar');

        $user->setAvatar($avatar, $bucket);

        return new UserResource($user);
    }

    /**
     * Devolve a conta, e não 204: tirar a foto enviada faz voltar a valer a do Google, e o
     * app precisa do link novo.
     *
     * @throws Throwable
     */
    public function destroyAvatar(#[CurrentUser] User $user): UserResource
    {
        $user->removeAvatar();

        return new UserResource($user);
    }
}
