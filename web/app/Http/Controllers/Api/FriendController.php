<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Friends\StoreFriendRequest;
use App\Http\Requests\Api\Friends\UpdateFriendRequest;
use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Throwable;

/**
 * Amizade: pedir, aceitar, bloquear e desfazer.
 */
final class FriendController
{
    public function index(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return FriendResource::collection(Friendship::listFor($user));
    }

    /**
     * @throws Throwable
     */
    public function store(StoreFriendRequest $request, #[CurrentUser] User $user): FriendResource
    {
        return new FriendResource(Friendship::requestByEmail($user, $request->string('email')->toString()));
    }

    /**
     * @throws Throwable
     */
    public function update(UpdateFriendRequest $request, Friendship $friendship, #[CurrentUser] User $user): FriendResource
    {
        match ($request->string('action')->toString()) {
            'accept' => $friendship->accept($user),
            default => $friendship->block($user),
        };

        $friendship->refresh();

        return new FriendResource($friendship->load(['requester', 'addressee']));
    }

    /**
     * @throws Throwable
     */
    public function destroy(Friendship $friendship, #[CurrentUser] User $user): Response
    {
        $friendship->remove($user);

        return response()->noContent();
    }
}
