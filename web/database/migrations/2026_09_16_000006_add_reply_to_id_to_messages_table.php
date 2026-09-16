<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * A mensagem que esta responde.
 *
 * `nullOnDelete` porque apagar a mensagem original não pode levar a resposta junto: o
 * fio da conversa continua fazendo sentido, e a resposta passa a aparecer sozinha.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('messages', function (Blueprint $table): void {
            $table->foreignId('reply_to_id')->nullable()->after('user_id')->constrained('messages')->nullOnDelete();
        });
    }

    public function down(): void
    {
        Schema::table('messages', function (Blueprint $table): void {
            $table->dropConstrainedForeignId('reply_to_id');
        });
    }
};
