<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Roles;

use App\Http\Requests\Api\Servers\StoreRoleRequest;
use App\Http\Resources\Api\RoleResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class StoreRoleController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreRoleRequest $request, Server $server, #[CurrentUser] User $user): RoleResource
    {
        $color = $request->string('color')->toString();

        $role = $server->createRole($user, $request->string('name')->toString(), $color === '' ? null : $color, $request->integer('permissions'));

        return new RoleResource($role);
    }
}
