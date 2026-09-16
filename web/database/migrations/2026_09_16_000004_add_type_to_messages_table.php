<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * O aviso de chegada é uma mensagem como as outras, só que de outro tipo: entra na mesma
 * paginação e no mesmo evento, e o texto quem monta é o app. O que já existe é `user`.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('messages', function (Blueprint $table): void {
            $table->string('type', 10)->default('user')->after('user_id');
        });
    }

    public function down(): void
    {
        Schema::table('messages', function (Blueprint $table): void {
            $table->dropColumn('type');
        });
    }
};
