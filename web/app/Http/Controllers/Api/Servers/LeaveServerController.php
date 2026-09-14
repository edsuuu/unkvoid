<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class LeaveServerController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Server $server, #[CurrentUser] User $user, SfuClient $sfu): Response
    {
        $server->leave($user, $sfu);

        return response()->noContent();
    }
}
