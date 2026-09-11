<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('releases', function (Blueprint $table): void {
            $table->id();
            $table->string('version', 20);
            $table->string('platform', 40);
            $table->string('file_name');
            $table->string('path');
            $table->unsignedBigInteger('size');
            $table->text('signature')->nullable();
            $table->text('notes')->nullable();
            $table->timestamp('published_at');
            $table->timestamps();

            $table->unique(['version', 'platform']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('releases');
    }
};
