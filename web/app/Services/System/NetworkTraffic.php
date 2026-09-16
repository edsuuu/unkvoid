<?php

declare(strict_types=1);

namespace App\Services\System;

use Illuminate\Support\Facades\Cache;

/**
 * Quanto está passando pela placa de rede da máquina. O kernel só conta bytes desde o
 * boot, então a taxa sai da diferença entre duas leituras — e a leitura anterior fica no
 * cache, e não na tela de quem abriu, para dois administradores verem o mesmo número.
 *
 * ponytail: só Linux (`/proc/net/dev`), que é onde a VPS roda. Em outro sistema a página
 * avisa em vez de mentir um número.
 */
final class NetworkTraffic
{
    private const string SOURCE = '/proc/net/dev';

    private const string CACHE_KEY = 'network:last-sample';

    private const int CACHE_SECONDS = 300;

    public function available(): bool
    {
        return is_readable(self::SOURCE);
    }

    /**
     * Bytes acumulados por placa, sem a de loopback: tráfego que não sai da máquina não é
     * tráfego de rede.
     *
     * @return array<string, array{received: int, sent: int}>
     */
    public function counters(): array
    {
        $content = $this->available() ? file_get_contents(self::SOURCE) : false;

        if ($content === false) {
            return [];
        }

        $counters = [];

        foreach (explode("\n", $content) as $line) {
            $parts = explode(':', $line, 2);

            if (count($parts) !== 2) {
                continue;
            }

            $name = mb_trim($parts[0]);
            $numbers = preg_split('/\s+/', mb_trim($parts[1])) ?: [];

            if ($name === 'lo' || count($numbers) < 9) {
                continue;
            }

            $counters[$name] = ['received' => (int) $numbers[0], 'sent' => (int) $numbers[8]];
        }

        return $counters;
    }

    /**
     * A taxa de cada placa desde a leitura anterior. Vem vazia na primeira vez: sem duas
     * leituras não há taxa nenhuma para mostrar.
     *
     * @return array<int, array{name: string, receivedMbps: float, sentMbps: float, receivedTotal: int, sentTotal: int}>
     */
    public function rates(): array
    {
        $now = microtime(true);
        $counters = $this->counters();
        $previous = $this->lastSample();

        Cache::put(self::CACHE_KEY, ['at' => $now, 'counters' => $counters], self::CACHE_SECONDS);

        if (is_null($previous) || $counters === []) {
            return [];
        }

        $seconds = max($now - $previous['at'], 0.001);
        $rates = [];

        foreach ($counters as $name => $counter) {
            $before = $previous['counters'][$name] ?? null;

            if (is_null($before)) {
                continue;
            }

            $rates[] = [
                'name' => $name,
                'receivedMbps' => $this->megabitsPerSecond($counter['received'] - $before['received'], $seconds),
                'sentMbps' => $this->megabitsPerSecond($counter['sent'] - $before['sent'], $seconds),
                'receivedTotal' => $counter['received'],
                'sentTotal' => $counter['sent'],
            ];
        }

        return $rates;
    }

    /**
     * O que voltou do cache pode ser qualquer coisa — outra versão do código, alguém
     * escrevendo na mesma chave — então só passa o que tem o formato esperado.
     *
     * @return array{at: float, counters: array<string, array{received: int, sent: int}>}|null
     */
    private function lastSample(): ?array
    {
        $sample = Cache::get(self::CACHE_KEY);

        if (! is_array($sample) || ! is_float($sample['at'] ?? null) || ! is_array($sample['counters'] ?? null)) {
            return null;
        }

        $counters = [];

        foreach ($sample['counters'] as $name => $counter) {
            if (is_string($name) && is_array($counter) && is_int($counter['received'] ?? null) && is_int($counter['sent'] ?? null)) {
                $counters[$name] = ['received' => $counter['received'], 'sent' => $counter['sent']];
            }
        }

        return ['at' => $sample['at'], 'counters' => $counters];
    }

    /**
     * Contador que anda para trás é placa que foi reiniciada: zero é mais honesto do que
     * uma taxa negativa.
     */
    private function megabitsPerSecond(int $bytes, float $seconds): float
    {
        return $bytes <= 0 ? 0.0 : round($bytes * 8 / 1_000_000 / $seconds, 2);
    }
}
