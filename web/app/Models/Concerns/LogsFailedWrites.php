<?php

declare(strict_types=1);

namespace App\Models\Concerns;

use App\Events\SfuEvent;
use App\Services\Sfu\SfuClient;
use Closure;
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
     * O tempo real sai pelo SFU. O evento continua sendo despachado dentro do Laravel: é
     * por ele que se observa o que foi publicado. O SFU fora do ar não desfaz o que já foi
     * gravado — o `publish()` engole a falha e a deixa no log do canal `sfu`.
     */
    protected static function publish(SfuEvent $event): void
    {
        event($event);

        $sfu = resolve(SfuClient::class);
        $payload = $event->payload();

        foreach ($event->channels() as $channel) {
            $sfu->publish($channel, $event->eventName(), $payload);
        }
    }
}
