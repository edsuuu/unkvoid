<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Roles;

use App\Http\Requests\Api\Servers\UpdateRoleRequest;
use App\Http\Resources\Api\RoleResource;
use App\Models\ServerRole;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class UpdateRoleController
{
    /**
     * @throws Throwable
     */
    public function __invoke(UpdateRoleRequest $request, ServerRole $role, #[CurrentUser] User $user): RoleResource
    {
        /** @var array{name?: string, color?: ?string, permissions?: int, position?: int} $changes */
        $changes = $request->validated();

        $role->change($user, $changes);

        return new RoleResource($role->refresh());
    }
}
