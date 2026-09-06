<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('channels', function (Blueprint $table): void {
            $table->uuid('id')->primary();
            $table->foreignUuid('server_id')->constrained()->cascadeOnDelete();
            $table->string('name');
            $table->enum('type', ['text', 'voice']);
            $table->unsignedInteger('position')->default(0);
            $table->timestamps();

            $table->index(['server_id', 'type', 'position']);
        });

        Schema::create('messages', function (Blueprint $table): void {
            $table->uuid('id')->primary();
            $table->foreignUuid('channel_id')->constrained()->cascadeOnDelete();
            $table->foreignUuid('user_id')->constrained('users')->cascadeOnDelete();
            $table->text('content');
            $table->timestamps();

            $table->index(['channel_id', 'id']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('messages');
        Schema::dropIfExists('channels');
    }
};
