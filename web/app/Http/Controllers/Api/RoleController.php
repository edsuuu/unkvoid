<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Servers\StoreRoleRequest;
use App\Http\Requests\Api\Servers\UpdateRoleRequest;
use App\Http\Resources\Api\RoleResource;
use App\Models\Server;
use App\Models\ServerRole;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

/**
 * Cargos de um servidor.
 */
final class RoleController
{
    /**
     * @throws Throwable
     */
    public function store(StoreRoleRequest $request, Server $server, #[CurrentUser] User $user): RoleResource
    {
        $color = $request->string('color')->toString();

        $role = $server->createRole($user, $request->string('name')->toString(), $color === '' ? null : $color, $request->integer('permissions'));

        return new RoleResource($role);
    }

    /**
     * @throws Throwable
     */
    public function update(UpdateRoleRequest $request, ServerRole $role, #[CurrentUser] User $user): RoleResource
    {
        /** @var array{name?: string, color?: ?string, permissions?: int, position?: int} $changes */
        $changes = $request->validated();

        $role->change($user, $changes);

        return new RoleResource($role->refresh());
    }

    /**
     * @throws Throwable
     */
    public function destroy(ServerRole $role, #[CurrentUser] User $user): Response
    {
        $role->remove($user);

        return response()->noContent();
    }
}
