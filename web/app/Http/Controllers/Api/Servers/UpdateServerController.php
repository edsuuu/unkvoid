<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Http\Requests\Api\Servers\UpdateServerRequest;
use App\Http\Resources\Api\ServerResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class UpdateServerController
{
    /**
     * @throws Throwable
     */
    public function __invoke(UpdateServerRequest $request, Server $server, #[CurrentUser] User $user): ServerResource
    {
        $server->rename($user, $request->string('name')->toString());

        return new ServerResource($server);
    }
}
