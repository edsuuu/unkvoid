<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Friends;

use App\Models\Friendship;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyFriendController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Friendship $friendship, #[CurrentUser] User $user): Response
    {
        $friendship->remove($user);

        return response()->noContent();
    }
}
