<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * O ícone do servidor mora no bucket privado, o mesmo das fotos de perfil, então a
 * coluna guarda só o caminho: a URL sai assinada e vencendo, nunca gravada.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('servers', function (Blueprint $table): void {
            $table->string('icon_path')->nullable()->after('invite_code');
        });
    }

    public function down(): void
    {
        Schema::table('servers', function (Blueprint $table): void {
            $table->dropColumn('icon_path');
        });
    }
};
