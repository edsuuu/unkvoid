<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Http\Requests\Api\Servers\StoreServerRequest;
use App\Http\Resources\Api\ServerResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class StoreServerController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreServerRequest $request, #[CurrentUser] User $user): ServerResource
    {
        return new ServerResource(Server::createFor($user, $request->string('name')->toString()));
    }
}
