<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Os convites como os do Discord: cada pessoa cria os seus, com validade e limite de usos,
     * e o convite leva para um canal. O `servers.invite_code` de antes continua valendo, para o
     * app já instalado não perder o convite. `invites_paused_at` pausa todos os do servidor.
     */
    public function up(): void
    {
        Schema::create('server_invites', function (Blueprint $table): void {
            $table->id();
            $table->foreignId('server_id')->constrained()->cascadeOnDelete();
            $table->foreignId('user_id')->nullable()->constrained()->nullOnDelete();
            $table->string('channel_id', 26)->nullable();
            $table->string('code', 16)->unique();
            $table->unsignedInteger('uses')->default(0);
            $table->unsignedInteger('max_uses')->nullable();
            $table->timestamp('expires_at')->nullable();
            $table->timestamps();

            $table->foreign('channel_id')->references('id')->on('channels')->nullOnDelete();
        });

        Schema::table('servers', function (Blueprint $table): void {
            $table->timestamp('invites_paused_at')->nullable()->after('invite_code');
        });
    }

    public function down(): void
    {
        Schema::table('servers', function (Blueprint $table): void {
            $table->dropColumn('invites_paused_at');
        });

        Schema::dropIfExists('server_invites');
    }
};
