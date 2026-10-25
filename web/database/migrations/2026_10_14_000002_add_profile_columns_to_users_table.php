<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * O cartão de perfil, como o do Discord: a bio, os pronomes, a cor da faixa (`#rrggbb`) e
     * a frase do status. Tudo opcional; sem cor, a faixa sai da cor do nome.
     */
    public function up(): void
    {
        Schema::table('users', function (Blueprint $table): void {
            $table->string('bio', 190)->nullable()->after('avatar_id');
            $table->string('pronouns', 40)->nullable()->after('bio');
            $table->string('banner_color', 7)->nullable()->after('pronouns');
            $table->string('custom_status', 128)->nullable()->after('banner_color');
        });
    }

    public function down(): void
    {
        Schema::table('users', function (Blueprint $table): void {
            $table->dropColumn(['bio', 'pronouns', 'banner_color', 'custom_status']);
        });
    }
};
