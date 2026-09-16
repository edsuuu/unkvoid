<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Friends;

use App\Http\Requests\Api\Friends\StoreFriendRequest;
use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class StoreFriendController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreFriendRequest $request, #[CurrentUser] User $user): FriendResource
    {
        return new FriendResource(Friendship::requestByEmail($user, $request->string('email')->toString()));
    }
}
