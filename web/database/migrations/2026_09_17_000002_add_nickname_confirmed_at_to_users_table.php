<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

/**
 * Quem se cadastra pelo app ou pelo Google ganha um apelido automático e o app pede que a
 * pessoa escolha o dela. Quem já tinha conta escolheu (ou aceitou) o apelido antes da
 * coluna existir, então nasce confirmado.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('users', function (Blueprint $table): void {
            $table->timestamp('nickname_confirmed_at')->nullable()->after('name');
        });

        DB::table('users')->whereNotNull('created_at')->update(['nickname_confirmed_at' => DB::raw('created_at')]);
        DB::table('users')->whereNull('created_at')->update(['nickname_confirmed_at' => now()]);
    }

    public function down(): void
    {
        Schema::table('users', function (Blueprint $table): void {
            $table->dropColumn('nickname_confirmed_at');
        });
    }
};
