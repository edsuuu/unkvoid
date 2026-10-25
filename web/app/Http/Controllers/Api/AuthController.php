<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Exceptions\InvalidCredentialsException;
use App\Http\Requests\Api\Auth\ExchangeAppCodeRequest;
use App\Http\Requests\Api\Auth\LoginRequest;
use App\Http\Requests\Api\Auth\LogoutRequest;
use App\Http\Requests\Api\Auth\RefreshRequest;
use App\Http\Requests\Api\Auth\RegisterRequest;
use App\Http\Requests\Api\Auth\StartRequest;
use App\Http\Resources\Api\AuthTokenResource;
use App\Models\User;
use App\Notifications\WelcomeNotification;
use App\Services\Auth\AppTokens;
use Illuminate\Http\Response;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Str;
use Illuminate\Validation\ValidationException;
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
        // O campo se chama `email` desde o app antigo, mas aceita o apelido também.
        $login = mb_strtolower(mb_trim($request->string('email')->toString()));
        $user = User::query()->where(User::loginColumn($login), $login)->first();

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
     * Entrar com o nome e uma senha: a conta nasce na hora (`User::createFromName`), sem
     * e-mail, e o app já fica logado. Dali em diante ela entra pelo apelido e a senha; o
     * e-mail vem depois, se a pessoa quiser (`POST /api/me/email`).
     *
     * @throws Throwable
     */
    public function start(StartRequest $request): AuthTokenResource
    {
        try {
            $user = User::createFromName(mb_trim($request->string('name')->toString()), $request->string('password')->toString());
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao criar a conta com o nome', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
            ]);

            throw $exception;
        }

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

    public function exchangeAppCode(ExchangeAppCodeRequest $request): AuthTokenResource
    {
        $code = $request->string('code')->toString();
        $key = 'app-login-exchange:'.hash('sha256', $code);
        $record = Cache::lock($key.':consume', 5)->block(2, fn (): mixed => Cache::pull($key));
        $verifier = $request->string('code_verifier')->toString();
        $challenge = mb_rtrim(strtr(base64_encode(hash('sha256', $verifier, true)), '+/', '-_'), '=');
        $storedChallenge = is_array($record) && is_string($record['challenge'] ?? null)
            ? $record['challenge']
            : null;

        if (is_null($storedChallenge) || ! hash_equals($storedChallenge, $challenge)) {
            throw ValidationException::withMessages(['code' => 'Este código de entrada expirou ou não é válido. Entre novamente.']);
        }

        $userId = $record['user_id'] ?? null;
        $user = is_int($userId) ? User::query()->find($userId) : null;

        if (! $user instanceof User) {
            throw ValidationException::withMessages(['code' => 'Este código de entrada expirou ou não é válido. Entre novamente.']);
        }

        return new AuthTokenResource($user, $this->tokens->issue($user, 'app', (bool) ($record['refresh'] ?? false)));
    }
}
