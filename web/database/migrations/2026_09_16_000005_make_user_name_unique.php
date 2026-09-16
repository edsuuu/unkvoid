<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

/**
 * O nome vira o apelido: único, como um @.
 *
 * Quem já tem conta não é renomeado — só quem colide. O desempate acrescenta um número ao
 * fim, na ordem de criação, para a conta mais antiga ficar com o apelido limpo.
 */
return new class extends Migration
{
    public function up(): void
    {
        foreach (DB::table('users')->select('name')->groupBy('name')->havingRaw('count(*) > 1')->pluck('name') as $name) {
            $repeated = DB::table('users')->where('name', $name)->orderBy('id')->pluck('id')->skip(1);

            foreach ($repeated as $position => $id) {
                DB::table('users')->where('id', $id)->update(['name' => mb_substr((string) $name, 0, 36).($position + 2)]);
            }
        }

        Schema::table('users', function (Blueprint $table): void {
            $table->unique('name');
        });
    }

    public function down(): void
    {
        Schema::table('users', function (Blueprint $table): void {
            $table->dropUnique(['name']);
        });
    }
};
