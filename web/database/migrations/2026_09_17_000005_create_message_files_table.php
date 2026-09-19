<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Uma mensagem pode levar mais de uma imagem, então a ligação com `files` é uma pivô. Não
     * guarda quem enviou: `files.user_id` já diz quem mandou o arquivo, e `messages.user_id`
     * quem escreveu a mensagem.
     */
    public function up(): void
    {
        Schema::create('message_files', function (Blueprint $table): void {
            $table->foreignId('message_id')->constrained()->cascadeOnDelete();
            $table->foreignId('file_id')->constrained()->cascadeOnDelete();
            $table->primary(['message_id', 'file_id']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('message_files');
    }
};
