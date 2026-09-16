<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Friends;

use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;

final class IndexFriendController
{
    public function __invoke(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return FriendResource::collection(Friendship::listFor($user));
    }
}
