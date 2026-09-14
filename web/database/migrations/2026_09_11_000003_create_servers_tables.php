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
            $table->id();
            $table->string('name', 100);
            $table->foreignId('owner_id')->constrained('users')->cascadeOnDelete();
            $table->string('invite_code', 10)->unique();
            $table->timestamps();
        });

        // `roles` já é do spatie/permission; o cargo do servidor mora em `server_roles`.
        Schema::create('server_roles', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('server_id')->constrained('servers')->cascadeOnDelete();
            $table->string('name', 100);
            $table->string('color', 7)->nullable();
            $table->unsignedInteger('position')->default(0);
            $table->unsignedBigInteger('permissions')->default(0);
            $table->boolean('is_everyone')->default(false);
            $table->timestamps();
        });

        Schema::create('server_members', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('server_id')->constrained('servers')->cascadeOnDelete();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->string('nickname', 32)->nullable();
            $table->boolean('server_mute')->default(false);
            $table->boolean('server_deaf')->default(false);
            $table->timestamp('joined_at');
            $table->timestamps();

            $table->unique(['server_id', 'user_id']);
        });

        Schema::create('member_roles', function (Blueprint $table): void {
            $table->foreignId('server_member_id')->constrained('server_members')->cascadeOnDelete();
            $table->foreignId('role_id')->constrained('server_roles')->cascadeOnDelete();

            $table->primary(['server_member_id', 'role_id']);
        });

        Schema::create('channels', function (Blueprint $table): void {
            $table->char('id', 26)->primary();
            $table->foreignId('server_id')->constrained('servers')->cascadeOnDelete();
            $table->string('name', 100);
            $table->string('type', 10);
            $table->string('topic', 1024)->nullable();
            $table->unsignedInteger('position')->default(0);
            $table->unsignedTinyInteger('user_limit')->nullable();
            $table->timestamps();
        });

        Schema::create('channel_overwrites', function (Blueprint $table): void {
            $table->id();
            $table->char('channel_id', 26);
            $table->string('target_type', 10);
            $table->unsignedBigInteger('target_id');
            $table->unsignedBigInteger('allow')->default(0);
            $table->unsignedBigInteger('deny')->default(0);
            $table->timestamps();

            $table->foreign('channel_id')->references('id')->on('channels')->cascadeOnDelete();
            $table->unique(['channel_id', 'target_type', 'target_id']);
        });

        Schema::create('messages', function (Blueprint $table): void {
            $table->id();
            $table->char('channel_id', 26);
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->text('body');
            $table->timestamp('edited_at')->nullable();
            $table->timestamps();

            $table->foreign('channel_id')->references('id')->on('channels')->cascadeOnDelete();
            $table->index(['channel_id', 'id']);
        });

        Schema::create('server_bans', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('server_id')->constrained('servers')->cascadeOnDelete();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->foreignId('banned_by')->nullable()->constrained('users')->nullOnDelete();
            $table->string('reason', 512)->nullable();
            $table->timestamps();

            $table->unique(['server_id', 'user_id']);
        });

        Schema::create('channel_accesses', function (Blueprint $table): void {
            $table->id();
            $table->char('channel_id', 26);
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->string('ip', 45);
            $table->string('sfu_ip', 45)->nullable();
            $table->string('user_agent', 1023)->nullable();
            $table->timestamp('joined_at');
            $table->timestamp('left_at')->nullable();
            $table->timestamps();

            $table->foreign('channel_id')->references('id')->on('channels')->cascadeOnDelete();
            $table->index(['channel_id', 'joined_at']);
            $table->index(['left_at', 'joined_at']);
            $table->index(['user_id', 'channel_id', 'left_at']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('channel_accesses');
        Schema::dropIfExists('server_bans');
        Schema::dropIfExists('messages');
        Schema::dropIfExists('channel_overwrites');
        Schema::dropIfExists('channels');
        Schema::dropIfExists('member_roles');
        Schema::dropIfExists('server_members');
        Schema::dropIfExists('server_roles');
        Schema::dropIfExists('servers');
    }
};
