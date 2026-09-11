<?php

declare(strict_types=1);

namespace App\Models;

use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Override;
use Throwable;

/**
 * @property int $id
 * @property string $fingerprint
 * @property string $signature
 * @property string $version
 * @property string $platform
 * @property string $log
 * @property int $occurrences
 * @property CarbonImmutable $first_seen_at
 * @property CarbonImmutable $last_seen_at
 */
#[Fillable(['fingerprint', 'signature', 'version', 'platform', 'log', 'occurrences', 'first_seen_at', 'last_seen_at'])]
final class ErrorReport extends Model
{
    /**
     * O que a política de privacidade promete guardar, e nada além disso.
     */
    private const int KEEP_DAYS = 90;

    /**
     * Guarda o relatório que chegou de um app instalado, somando no erro que já existe.
     *
     * O mesmo pânico em dez computadores é um problema só, e não dez: por isso a chave é
     * a impressão digital do erro, e não o envio. O log guardado é sempre o do último
     * envio, que é o que tem mais chance de ainda fazer sentido.
     *
     * @throws Throwable
     */
    public static function record(string $version, string $platform, string $log): self
    {
        $signature = self::signatureOf($log);
        $fingerprint = hash('sha256', implode('|', [$version, $platform, $signature]));

        try {
            return DB::transaction(function () use ($fingerprint, $signature, $version, $platform, $log): self {
                $report = self::query()->firstOrCreate(
                    ['fingerprint' => $fingerprint],
                    [
                        'signature' => $signature,
                        'version' => $version,
                        'platform' => $platform,
                        'log' => $log,
                        'occurrences' => 0,
                        'first_seen_at' => now(),
                        'last_seen_at' => now(),
                    ],
                );

                $report->update(['log' => $log, 'last_seen_at' => now()]);
                $report->increment('occurrences');

                // A limpeza anda junto com o que chega, e não num agendador: a VPS não
                // roda `schedule:run`, e um prazo que ninguém cumpre é pior do que
                // nenhum — ele está escrito na política de privacidade.
                self::query()->where('last_seen_at', '<', now()->subDays(self::KEEP_DAYS))->delete();

                return $report->refresh();
            });
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao guardar o relatório de erro do app', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'version' => $version,
                'platform' => $platform,
            ]);

            throw $exception;
        }
    }

    /**
     * A linha que dá nome ao erro: o primeiro pânico, ou o primeiro ERROR.
     *
     * Os números saem fora antes de virar chave. Sem isso, a hora do relógio e o
     * endereço de memória fariam a mesma falha, em dois computadores, virar dois erros
     * diferentes — e mapear deixaria de funcionar exatamente quando começa a importar.
     */
    public static function signatureOf(string $log): string
    {
        foreach (explode("\n", $log) as $line) {
            if (! str_contains($line, 'panic ') && ! str_contains($line, 'ERROR')) {
                continue;
            }

            return mb_substr(mb_trim((string) preg_replace('/\d+/', '#', $line)), 0, 250);
        }

        return 'erro sem linha reconhecida';
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'occurrences' => 'integer',
            'first_seen_at' => 'datetime',
            'last_seen_at' => 'datetime',
        ];
    }
}
