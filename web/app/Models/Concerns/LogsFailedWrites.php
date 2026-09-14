<?php

declare(strict_types=1);

namespace App\Models\Concerns;

use Closure;
use Illuminate\Broadcasting\BroadcastException;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Throwable;

trait LogsFailedWrites
{
    /**
     * Toda escrita dos servidores passa por aqui: transação para o conjunto, e a falha
     * deixa rastro no log antes de subir.
     *
     * @template TReturn
     *
     * @param  Closure(): TReturn  $write
     * @param  array<string, mixed>  $context
     * @return TReturn
     *
     * @throws Throwable
     */
    protected static function write(string $failure, Closure $write, array $context = []): mixed
    {
        try {
            return DB::transaction($write);
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] '.$failure, [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                ...$context,
            ]);

            throw $exception;
        }
    }

    /**
     * O Reverb fora do ar não desfaz o que já foi gravado: quem chamou recebe a resposta
     * normal, e a falha fica no log.
     */
    protected static function broadcast(object $event): void
    {
        try {
            event($event);
        } catch (BroadcastException $exception) {
            Log::channel('daily')->error('[ERRO] o Reverb não respondeu', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'event' => $event::class,
            ]);
        }
    }
}
