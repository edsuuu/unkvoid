<?php

declare(strict_types=1);

namespace App\Services\Auth;

use App\Exceptions\InvalidRefreshTokenException;
use App\Models\User;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Laravel\Sanctum\PersonalAccessToken;
use Throwable;

/**
 * Os tokens do app. Quem pede renovação recebe dois: o de acesso, que vence em um dia, e o de
 * renovação, que vence em sessenta e só serve para trocar o par em `POST /api/auth/refresh`.
 * Quem não pede — o app do Tauri até a 0.0.40 — recebe o token de sempre, sem validade: ele
 * não sabe renovar, e um token vencido o deixaria preso fora da conta.
 */
final readonly class AppTokens
{
    public const string ACCESS = 'app';

    public const string REFRESH = 'refresh';

    private const int ACCESS_HOURS = 24;

    private const int REFRESH_DAYS = 60;

    /**
     * @return array{token: string, refresh_token: string|null, expires_at: string|null}
     */
    public function issue(User $user, string $device, bool $renewable): array
    {
        if (! $renewable) {
            return ['token' => $user->createToken($device)->plainTextToken, 'refresh_token' => null, 'expires_at' => null];
        }

        $expiresAt = now()->addHours(self::ACCESS_HOURS);

        return [
            'token' => $user->createToken($device, [self::ACCESS], $expiresAt)->plainTextToken,
            'refresh_token' => $user->createToken($device, [self::REFRESH], now()->addDays(self::REFRESH_DAYS))->plainTextToken,
            'expires_at' => $expiresAt->toIso8601String(),
        ];
    }

    /**
     * Troca o token de renovação por um par novo. O usado morre na hora: se alguém o copiou e
     * usou antes, o dono cai no login na próxima renovação — e sair da conta derruba o par
     * de quem copiou.
     *
     * @return array{user: User, token: string, refresh_token: string|null, expires_at: string|null}
     *
     * @throws Throwable
     */
    public function renew(string $refreshToken): array
    {
        $stored = PersonalAccessToken::findToken($refreshToken);
        $user = $stored?->tokenable;

        throw_if(
            is_null($stored)
            || ! $user instanceof User
            || ! in_array(self::REFRESH, $stored->abilities ?? [], true)
            || is_null($stored->expires_at)
            || $stored->expires_at->isPast(),
            InvalidRefreshTokenException::class,
        );

        try {
            return DB::transaction(function () use ($stored, $user): array {
                $stored->delete();

                return ['user' => $user, ...$this->issue($user, $stored->name, true)];
            });
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao renovar o token do app', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'user_id' => $user->id,
            ]);

            throw $exception;
        }
    }

    /**
     * Sair da conta derruba o token de renovação junto com o de acesso — só o da própria
     * pessoa, e só se for mesmo de renovação.
     */
    public function revoke(User $user, ?string $refreshToken): void
    {
        if (is_null($refreshToken)) {
            return;
        }

        $stored = PersonalAccessToken::findToken($refreshToken);

        if (is_null($stored) || ! $stored->tokenable?->is($user) || ! in_array(self::REFRESH, $stored->abilities ?? [], true)) {
            return;
        }

        $stored->delete();
    }
}
