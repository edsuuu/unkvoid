<?php

declare(strict_types=1);

namespace Database\Seeders;

use App\Models\User;
use Illuminate\Database\Seeder;
use Illuminate\Support\Facades\Hash;

final class Seeder002AdminUser extends Seeder
{
    /**
     * Run the database seeds.
     */
    public function run(): void
    {
        $admin = User::query()->updateOrCreate(['email' => 'admin@admin.com'], [
            'name' => 'Administrador',
            'password' => Hash::make('123'),
            'email_verified_at' => now(),
        ]);

        $admin->assignRole('Administrador');
    }
}
