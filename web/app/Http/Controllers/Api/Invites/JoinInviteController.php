<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Invites;

use App\Http\Resources\Api\ServerResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class JoinInviteController
{
    /**
     * @throws Throwable
     */
    public function __invoke(string $code, #[CurrentUser] User $user): ServerResource
    {
        return new ServerResource(Server::joinByInvite($user, $code));
    }
}
