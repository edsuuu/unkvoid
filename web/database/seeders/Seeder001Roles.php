<?php

declare(strict_types=1);

namespace Database\Seeders;

use App\Models\User;
use Illuminate\Database\Seeder;
use Spatie\Permission\Models\Role;

final class Seeder001Roles extends Seeder
{
    public function run(): void
    {
        Role::query()->firstOrCreate(['name' => User::ROLE_ADMIN]);
    }
}
