<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * A foto enviada aponta para `files`; o `avatar_url` continua sendo o link permanente do
     * Google, que vale enquanto ninguém enviar foto.
     */
    public function up(): void
    {
        Schema::table('users', function (Blueprint $table): void {
            $table->foreignId('avatar_id')->nullable()->after('avatar_url')->constrained('files')->nullOnDelete();
        });
    }

    public function down(): void
    {
        Schema::table('users', function (Blueprint $table): void {
            $table->dropConstrainedForeignId('avatar_id');
        });
    }
};
