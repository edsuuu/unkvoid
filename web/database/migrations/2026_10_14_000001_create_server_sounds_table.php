<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Os sons do soundboard de cada servidor. O arquivo mora no bucket privado, como o ícone;
     * `user_id` é quem enviou e fica nulo se a conta for apagada, para o som não sumir junto.
     */
    public function up(): void
    {
        Schema::create('server_sounds', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('server_id')->constrained()->cascadeOnDelete();
            $table->foreignId('user_id')->nullable()->constrained()->nullOnDelete();
            $table->string('name', 32);
            $table->string('emoji', 32)->nullable();
            $table->string('path');
            $table->unsignedTinyInteger('volume')->default(100);
            $table->timestamps();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('server_sounds');
    }
};
