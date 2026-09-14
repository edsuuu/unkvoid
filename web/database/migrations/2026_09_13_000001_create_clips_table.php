<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('clips', function (Blueprint $table): void {
            $table->char('id', 26)->primary();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->foreignId('streamer_user_id')->nullable()->constrained('users')->nullOnDelete();
            $table->char('channel_id', 26)->nullable();
            $table->string('streamer_name');
            $table->string('server_name', 100);
            $table->string('channel_name', 100);
            $table->string('status', 10);
            $table->unsignedInteger('duration_ms')->nullable();
            $table->unsignedBigInteger('size_bytes')->nullable();
            $table->timestamps();

            $table->foreign('channel_id')->references('id')->on('channels')->nullOnDelete();
            $table->index(['user_id', 'created_at']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('clips');
    }
};
