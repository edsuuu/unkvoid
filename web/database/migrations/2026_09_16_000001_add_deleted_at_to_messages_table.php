<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * Apagar mensagem passa a ser soft delete: ela some da conversa e continua no banco.
 *
 * Sem isto, moderar uma ofensa apagava a prova junto — e a auditoria do servidor só
 * registra que alguém apagou, não o que estava escrito.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('messages', function (Blueprint $table): void {
            $table->softDeletes();
        });
    }

    public function down(): void
    {
        Schema::table('messages', function (Blueprint $table): void {
            $table->dropSoftDeletes();
        });
    }
};
