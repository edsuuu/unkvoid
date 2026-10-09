<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * A categoria é um canal (`type = category`) e os canais dela apontam para ela.
 *
 * `nullOnDelete` porque apagar a categoria não pode levar os canais junto: eles voltam
 * para a raiz da lista.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('channels', function (Blueprint $table): void {
            $table->char('parent_id', 26)->nullable()->after('server_id');
            $table->foreign('parent_id')->references('id')->on('channels')->nullOnDelete();
        });
    }

    public function down(): void
    {
        Schema::table('channels', function (Blueprint $table): void {
            $table->dropConstrainedForeignId('parent_id');
        });
    }
};
