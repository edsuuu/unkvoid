<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * As duas chaves do editor de cargo do Discord: `hoist` mostra quem tem o cargo num grupo
     * próprio na lista de membros; `mentionable` deixa qualquer um @mencionar o cargo.
     */
    public function up(): void
    {
        Schema::table('server_roles', function (Blueprint $table): void {
            $table->boolean('hoist')->default(false)->after('permissions');
            $table->boolean('mentionable')->default(false)->after('hoist');
        });
    }

    public function down(): void
    {
        Schema::table('server_roles', function (Blueprint $table): void {
            $table->dropColumn(['hoist', 'mentionable']);
        });
    }
};
