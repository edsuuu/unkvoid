<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Friends;

use App\Http\Requests\Api\Friends\UpdateFriendRequest;
use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class UpdateFriendController
{
    /**
     * @throws Throwable
     */
    public function __invoke(UpdateFriendRequest $request, Friendship $friendship, #[CurrentUser] User $user): FriendResource
    {
        match ($request->string('action')->toString()) {
            'accept' => $friendship->accept($user),
            default => $friendship->block($user),
        };

        $friendship->refresh();

        return new FriendResource($friendship->load(['requester', 'addressee']));
    }
}
