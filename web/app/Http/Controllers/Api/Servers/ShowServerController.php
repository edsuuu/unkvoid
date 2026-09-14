<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Http\Resources\Api\ServerTreeResource;
use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class ShowServerController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Server $server, #[CurrentUser] User $user, SfuClient $sfu): ServerTreeResource
    {
        return new ServerTreeResource($server, $server->memberOrFail($user), $sfu);
    }
}
