<?php

declare(strict_types=1);

namespace App\Services\Sfu;

use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\Config;
use Illuminate\Http\Client\PendingRequest;
use Illuminate\Http\Client\Response;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;
use JsonException;
use Throwable;

/**
 * As conversas com o SFU: o token que o app apresenta no `join`, e as chamadas HTTP
 * assinadas com o mesmo segredo. HMAC e não JWT de propósito, como do lado de lá.
 */
final readonly class SfuClient
{
    public const int TOKEN_SECONDS = 60;

    private const int PRESENCE_CACHE_SECONDS = 3;

    private const string PRESENCE_CACHE_KEY = 'sfu:presence';

    public function __construct(
        #[Config('services.sfu.url')] private string $url,
        #[Config('services.sfu.secret')] private string $secret,
    ) {}

    /**
     * @param  array<int, string>  $can
     *
     * @throws JsonException
     */
    public function token(Channel $channel, User $user, array $can): string
    {
        return $this->sign($channel->id, $user, $can);
    }

    /**
     * A sala por código não existe em banco: o token só carrega o código. Quem entra
     * logado leva a conta junto, e é isso que faz o SFU derrubar o outro dispositivo.
     *
     * @throws JsonException
     */
    public function roomToken(string $code, User $user): string
    {
        return $this->sign($code, $user, ['speak', 'stream', 'video']);
    }

    public function kick(Channel $channel, string $subject): int
    {
        $kicked = $this->post("/rooms/{$channel->id}/kick", ['userId' => $subject])['kicked'] ?? 0;

        return is_int($kicked) ? $kicked : 0;
    }

    public function mute(Channel $channel, string $subject, bool $muted): int
    {
        $mutedCount = $this->post("/rooms/{$channel->id}/mute", ['userId' => $subject, 'muted' => $muted])['muted'] ?? 0;

        return is_int($mutedCount) ? $mutedCount : 0;
    }

    /**
     * Quem está em cada sala: cache de 3 s entre requests e uma chamada só dentro do
     * mesmo request, mesmo que a árvore pergunte canal por canal. `fresh` esquece o cache
     * antes, para quem precisa contar de verdade.
     *
     * @return array<string, array<int, array{sub: string, name: string, sources: array<int, string>}>> por sala
     */
    public function presence(bool $fresh = false): array
    {
        return once(function () use ($fresh): array {
            if ($fresh) {
                Cache::forget(self::PRESENCE_CACHE_KEY);
            }

            /** @var array<string, array<int, array{sub: string, name: string, sources: array<int, string>}>> $rooms */
            $rooms = Cache::remember(self::PRESENCE_CACHE_KEY, self::PRESENCE_CACHE_SECONDS, fn (): array => (array) ($this->send('GET', '/presence')['rooms'] ?? []));

            return $rooms;
        });
    }

    /**
     * @return array<int, array{sub: string, name: string, sources: array<int, string>}>
     */
    public function peers(Channel $channel, bool $fresh = false): array
    {
        return $this->presence($fresh)[$channel->id] ?? [];
    }

    /**
     * @param  array<int, string>  $can
     *
     * @throws JsonException
     */
    private function sign(string $room, User $user, array $can): string
    {
        $claims = json_encode([
            'room' => $room,
            'sub' => $user->subject(),
            'name' => $user->name,
            'exp' => time() + self::TOKEN_SECONDS,
            'can' => $can,
        ], JSON_THROW_ON_ERROR);

        $body = mb_rtrim(strtr(base64_encode($claims), '+/', '-_'), '=');

        return $body.'.'.hash_hmac('sha256', $body, $this->secret);
    }

    /**
     * @param  array<string, mixed>  $data
     * @return array<string, mixed>
     */
    private function post(string $path, array $data): array
    {
        return $this->send('POST', $path, $data);
    }

    /**
     * O SFU fora do ar não derruba a ação de quem chamou: quem foi expulso do servidor
     * continua expulso, e a falha fica no log do canal `sfu`.
     *
     * @param  array<string, mixed>|null  $data
     * @return array<string, mixed>
     */
    private function send(string $method, string $path, ?array $data = null): array
    {
        $response = $this->call($method, $path, $data);

        if (is_null($response) || $response->failed()) {
            return [];
        }

        /** @var array<string, mixed> $json */
        $json = (array) $response->json();

        return $json;
    }

    /**
     * @param  array<string, mixed>|null  $data
     */
    private function call(string $method, string $path, ?array $data = null): ?Response
    {
        $body = '';

        try {
            if (! is_null($data)) {
                $body = json_encode($data, JSON_THROW_ON_ERROR);
            }

            $timestamp = (string) time();
            $headers = [
                'X-Unkvoid-Timestamp' => $timestamp,
                'X-Unkvoid-Signature' => hash_hmac('sha256', $timestamp.PHP_EOL.$method.PHP_EOL.$path.PHP_EOL.$body, $this->secret),
            ];

            $response = $this->request($method, $headers, $body)->send($method, $this->url.$path);

            Log::channel('sfu')->info('[INFO] chamada ao SFU', [
                'method' => $method,
                'path' => $path,
                'headers' => [...$headers, 'X-Unkvoid-Signature' => '***'],
                'body' => $body,
                'status' => $response->status(),
                'response' => $response->body(),
            ]);

            if ($response->failed()) {
                Log::channel('sfu')->error('[ERRO] o SFU recusou a chamada', [
                    'method' => $method,
                    'path' => $path,
                    'status' => $response->status(),
                ]);
            }

            return $response;
        } catch (Throwable $exception) {
            Log::channel('sfu')->error('[ERRO] o SFU não respondeu', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'method' => $method,
                'path' => $path,
                'body' => $body,
            ]);

            return null;
        }
    }

    /**
     * Só o que muda alguém de lugar (kick, mute) tenta de novo: a presença é consulta, e
     * esperar por ela segura a árvore inteira.
     *
     * @param  array<string, string>  $headers
     */
    private function request(string $method, array $headers, string $body): PendingRequest
    {
        $request = $method === 'GET'
            ? Http::timeout(2)
            : Http::timeout(5)->retry(2, 200, throw: false);

        $request = $request->withHeaders($headers)->acceptJson();

        if ($body === '') {
            return $request;
        }

        return $request->withBody($body, 'application/json');
    }
}
