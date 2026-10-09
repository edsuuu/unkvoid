<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * Até onde cada pessoa leu cada canal: o id da última mensagem vista. Sem linha, nunca
 * leu. `last_read_id` não é chave estrangeira de propósito: a mensagem pode ser apagada e
 * a marca continua valendo.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::create('channel_reads', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->char('channel_id', 26);
            $table->unsignedBigInteger('last_read_id')->default(0);
            $table->timestamps();

            $table->foreign('channel_id')->references('id')->on('channels')->cascadeOnDelete();
            $table->unique(['user_id', 'channel_id']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('channel_reads');
    }
};
