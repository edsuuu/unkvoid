<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('servers', function (Blueprint $table): void {
            $table->uuid('id')->primary();
            $table->foreignUuid('owner_id')->constrained('users')->cascadeOnDelete();
            $table->string('name');
            $table->string('icon_path')->nullable();
            $table->string('invite_code', 12)->unique();
            $table->timestamps();
        });

        Schema::create('server_members', function (Blueprint $table): void {
            $table->uuid('id')->primary();
            $table->foreignUuid('server_id')->constrained()->cascadeOnDelete();
            $table->foreignUuid('user_id')->constrained('users')->cascadeOnDelete();
            $table->string('nickname')->nullable();
            $table->enum('role', ['owner', 'admin', 'member'])->default('member');
            $table->timestamp('joined_at')->useCurrent();

            $table->unique(['server_id', 'user_id']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('server_members');
        Schema::dropIfExists('servers');
    }
};
