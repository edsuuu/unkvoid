<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Bans;

use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyBanController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Server $server, User $user, #[CurrentUser] User $actor): Response
    {
        $server->unban($actor, $user);

        return response()->noContent();
    }
}
