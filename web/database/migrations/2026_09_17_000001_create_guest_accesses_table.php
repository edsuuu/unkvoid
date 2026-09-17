<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * Quem entrou numa sala por código, sem conta. Não há usuário para ligar: o que identifica
 * é o id da instalação que o app sorteia, o nome digitado e o IP que o SFU viu. A sala
 * também não tem tabela — o código só existe enquanto alguém está nela.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::create('guest_accesses', function (Blueprint $table): void {
            $table->id();
            $table->string('room', 32);
            $table->string('install_id', 64);
            $table->string('name', 40);
            $table->string('ip', 45);
            $table->timestamp('joined_at');
            $table->timestamp('left_at')->nullable();
            $table->timestamps();

            $table->index(['room', 'joined_at']);
            $table->index(['left_at', 'joined_at']);
            $table->index(['install_id', 'room', 'left_at']);
            $table->index('joined_at');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('guest_accesses');
    }
};
