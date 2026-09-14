<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyServerController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Server $server, #[CurrentUser] User $user): Response
    {
        $server->destroyBy($user);

        return response()->noContent();
    }
}
