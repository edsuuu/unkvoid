<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('error_reports', function (Blueprint $table): void {
            $table->id();
            $table->string('fingerprint', 64)->unique();
            $table->string('signature');
            $table->string('version', 20);
            $table->string('platform', 20);
            $table->text('log');
            $table->unsignedInteger('occurrences')->default(0);
            $table->timestamp('first_seen_at');
            $table->timestamp('last_seen_at');
            $table->timestamps();

            $table->index(['platform', 'last_seen_at']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('error_reports');
    }
};
