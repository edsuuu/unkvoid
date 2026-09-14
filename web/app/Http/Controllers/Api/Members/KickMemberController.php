<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Members;

use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class KickMemberController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Server $server, User $user, #[CurrentUser] User $actor, SfuClient $sfu): Response
    {
        $server->kick($actor, $user, $sfu);

        return response()->noContent();
    }
}
