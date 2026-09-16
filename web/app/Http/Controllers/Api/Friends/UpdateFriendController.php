<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Friends;

use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Request;
use Illuminate\Validation\ValidationException;
use Throwable;

final class UpdateFriendController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Request $request, Friendship $friendship, #[CurrentUser] User $user): FriendResource
    {
        $action = $request->string('action')->toString();

        match ($action) {
            'accept' => $friendship->accept($user),
            'block' => $friendship->block($user),
            default => throw ValidationException::withMessages(['action' => 'Ação desconhecida.']),
        };

        $friendship->refresh();

        return new FriendResource($friendship->load(['requester', 'addressee']));
    }
}
