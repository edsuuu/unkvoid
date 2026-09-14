<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Http\Resources\Api\InviteResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class RegenerateInviteController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Server $server, #[CurrentUser] User $user): InviteResource
    {
        return new InviteResource($server->regenerateInvite($user));
    }
}
