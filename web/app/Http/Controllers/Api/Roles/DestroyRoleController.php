<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Roles;

use App\Models\ServerRole;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyRoleController
{
    /**
     * @throws Throwable
     */
    public function __invoke(ServerRole $role, #[CurrentUser] User $user): Response
    {
        $role->remove($user);

        return response()->noContent();
    }
}
