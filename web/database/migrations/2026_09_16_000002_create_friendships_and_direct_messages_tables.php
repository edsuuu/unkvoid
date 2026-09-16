<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * Amigos e conversa direta.
 *
 * `friendships` guarda uma linha por pedido, na direção em que ele foi feito: quem pediu,
 * para quem, e em que pé está. O par é único, então pedir de novo não cria uma segunda
 * linha, e bloquear é a mesma linha mudando de situação.
 *
 * `direct_messages` não tem tabela de conversa porque conversa direta é sempre entre
 * dois: o par de pessoas já identifica o fio, e a lista da Home sai agrupando por ele.
 * Vira `conversations` no dia em que existir conversa em grupo.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::create('friendships', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('requester_id')->constrained('users')->cascadeOnDelete();
            $table->foreignId('addressee_id')->constrained('users')->cascadeOnDelete();
            $table->string('status', 10)->default('pending');
            $table->timestamp('responded_at')->nullable();
            $table->timestamps();

            $table->unique(['requester_id', 'addressee_id']);
            $table->index(['addressee_id', 'status']);
        });

        Schema::create('direct_messages', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('sender_id')->constrained('users')->cascadeOnDelete();
            $table->foreignId('recipient_id')->constrained('users')->cascadeOnDelete();
            $table->text('body');
            $table->timestamp('edited_at')->nullable();
            $table->timestamp('read_at')->nullable();
            $table->softDeletes();
            $table->timestamps();

            // A conversa é lida sempre pelos dois lados do par, e a contagem de não lidas
            // sai do segundo índice sem tocar no corpo das mensagens.
            $table->index(['sender_id', 'recipient_id', 'id']);
            $table->index(['recipient_id', 'read_at']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('direct_messages');
        Schema::dropIfExists('friendships');
    }
};
