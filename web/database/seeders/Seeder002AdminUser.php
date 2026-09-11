<?php

declare(strict_types=1);

namespace Database\Seeders;

use App\Models\User;
use Illuminate\Database\Seeder;
use Illuminate\Support\Facades\Config;

final class Seeder002AdminUser extends Seeder
{
    public function run(): void
    {
        $email = mb_strtolower(mb_trim(Config::string('unkvoid.admin_email')));

        if ($email === '') {
            return;
        }

        $admin = User::query()->firstOrCreate(['email' => $email], [
            'name' => 'Administrador',
            'email_verified_at' => now(),
        ]);

        $admin->assignRole(User::ROLE_ADMIN);
    }
}
