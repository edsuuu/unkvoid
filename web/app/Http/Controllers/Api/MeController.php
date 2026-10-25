<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Auth\StoreAvatarRequest;
use App\Http\Requests\Api\Auth\StoreEmailRequest;
use App\Http\Requests\Api\Auth\UpdateMeRequest;
use App\Http\Resources\Api\UserResource;
use App\Models\User;
use App\Services\Storage\BucketService;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\UploadedFile;
use Illuminate\Validation\ValidationException;
use Throwable;

/**
 * A conta de quem está logado: o apelido, o perfil, a foto e o e-mail e senha de quem entrou só
 * com o nome.
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
        if ($request->has('name')) {
            $user->changeNickname($request->string('name')->toString());
        }

        if ($request->has('status')) {
            $minutes = is_null($request->input('status_minutes')) ? null : $request->integer('status_minutes');
            $user->changeStatus($request->string('status')->toString(), $minutes);
        }

        /** @var array{bio?: ?string, pronouns?: ?string, banner_color?: ?string, custom_status?: ?string} $profile */
        $profile = $request->safe()->only(['bio', 'pronouns', 'banner_color', 'custom_status']);

        $user->changeProfile($profile);

        return new UserResource($user);
    }

    /**
     * O e-mail para a conta que nasceu com o nome. Uma vez só: quem já tem e-mail de verdade
     * troca pelo site, com a senha de antes.
     *
     * @throws Throwable
     */
    public function storeEmail(StoreEmailRequest $request, #[CurrentUser] User $user): UserResource
    {
        throw_unless($user->hasPlaceholderEmail(), ValidationException::withMessages(['email' => 'Esta conta já tem e-mail.']));

        $user->addEmail(mb_strtolower(mb_trim($request->string('email')->toString())));

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
