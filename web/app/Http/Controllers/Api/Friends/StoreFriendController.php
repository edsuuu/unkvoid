<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Friends;

use App\Http\Requests\Api\Friends\StoreFriendRequest;
use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Validation\ValidationException;
use Throwable;

final class StoreFriendController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreFriendRequest $request, #[CurrentUser] User $user): FriendResource
    {
        $email = mb_strtolower($request->string('email')->trim()->toString());
        $target = User::query()->where('email', $email)->first();

        // Mensagem igual para "não existe" e para "é você": responder diferente contaria a
        // quem procura se aquele e-mail tem conta aqui.
        if (is_null($target)) {
            throw ValidationException::withMessages(['email' => 'Ninguém com esse e-mail.']);
        }

        return new FriendResource(Friendship::request($user, $target)->load(['requester', 'addressee']));
    }
}
