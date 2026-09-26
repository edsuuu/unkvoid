<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Exceptions\InvalidCredentialsException;
use App\Http\Requests\Api\Auth\LoginRequest;
use App\Http\Requests\Api\Auth\LogoutRequest;
use App\Http\Requests\Api\Auth\RefreshRequest;
use App\Http\Requests\Api\Auth\RegisterRequest;
use App\Http\Resources\Api\AuthTokenResource;
use App\Models\User;
use App\Notifications\WelcomeNotification;
use App\Services\Auth\AppTokens;
use Illuminate\Http\Response;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Str;
use Laravel\Sanctum\PersonalAccessToken;
use Throwable;

/**
 * Entrar, criar conta, renovar a sessão e sair, do app. O site tem as telas dele em Livewire.
 */
final readonly class AuthController
{
    public function __construct(
        private AppTokens $tokens,
    ) {}

    /**
     * @throws InvalidCredentialsException
     */
    public function login(LoginRequest $request): AuthTokenResource
    {
        $email = mb_strtolower(mb_trim($request->string('email')->toString()));
        $user = User::query()->where('email', $email)->first();

        throw_if(is_null($user) || is_null($user->password) || ! Hash::check($request->string('password')->toString(), $user->password), InvalidCredentialsException::class);

        $device = $request->string('device')->toString();
        $user->notifyNewLoginIfUnknown('app: '.$device, (string) $request->ip(), (string) $request->userAgent());

        return new AuthTokenResource($user, $this->tokens->issue($user, $device, $request->boolean('refresh')));
    }

    /**
     * O app não pede apelido no cadastro: a conta nasce com um tirado do e-mail, sem
     * confirmar, e o app abre a escolha do apelido até a pessoa decidir (`PATCH /api/me`).
     *
     * @throws Throwable
     */
    public function register(RegisterRequest $request): AuthTokenResource
    {
        $email = mb_strtolower(mb_trim($request->string('email')->toString()));

        try {
            $user = User::query()->create([
                'name' => User::freeNickname(Str::before($email, '@')),
                'email' => $email,
                'password' => $request->string('password')->toString(),
            ]);
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao criar a conta pela API', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'email' => $email,
            ]);

            throw $exception;
        }

        $user->notifyQuietly(new WelcomeNotification);

        return new AuthTokenResource($user, $this->tokens->issue($user, $request->string('device')->toString(), $request->boolean('refresh')));
    }

    /**
     * Troca o token de renovação por um par novo. Sem conta na requisição: quem chega aqui é
     * justamente o app cujo token de acesso venceu.
     *
     * @throws Throwable
     */
    public function refresh(RefreshRequest $request): AuthTokenResource
    {
        $renewed = $this->tokens->renew($request->string('refresh_token')->toString());

        return new AuthTokenResource($renewed['user'], $renewed);
    }

    public function logout(LogoutRequest $request): Response
    {
        $user = $request->user();
        $token = $user?->currentAccessToken();

        if ($token instanceof PersonalAccessToken) {
            $token->delete();
        }

        if ($user instanceof User) {
            $this->tokens->revoke($user, $request->filled('refresh_token') ? $request->string('refresh_token')->toString() : null);
        }

        return response()->noContent();
    }
}
