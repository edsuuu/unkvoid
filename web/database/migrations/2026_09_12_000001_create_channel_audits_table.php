<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * O histórico do canal mora aqui, e não na tabela `audits` do pacote, porque lá a
     * coluna do id é numérica e o id do canal é um ULID de 26 letras — o MySQL truncava
     * e derrubava a transação que criava o canal. As colunas repetem o formato da
     * `audits` de propósito: a tela de auditoria junta as duas listas sem converter nada.
     */
    public function up(): void
    {
        Schema::create('channel_audits', function (Blueprint $table): void {
            $table->id();
            $table->char('channel_id', 26);
            $table->foreignId('server_id')->constrained()->cascadeOnDelete();
            $table->foreignId('user_id')->nullable()->constrained()->nullOnDelete();
            $table->string('event', 20);
            $table->json('old_values')->nullable();
            $table->json('new_values')->nullable();
            $table->string('ip_address', 45)->nullable();
            $table->string('user_agent', 1023)->nullable();
            $table->timestamp('created_at');

            $table->index(['channel_id', 'id']);
            $table->index(['server_id', 'created_at']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('channel_audits');
    }
};
