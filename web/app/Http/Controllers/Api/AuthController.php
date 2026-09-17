<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Exceptions\InvalidCredentialsException;
use App\Http\Requests\Api\Auth\LoginRequest;
use App\Http\Requests\Api\Auth\RegisterRequest;
use App\Http\Resources\Api\AuthTokenResource;
use App\Models\User;
use App\Notifications\NewLoginNotification;
use App\Notifications\WelcomeNotification;
use Illuminate\Http\Request;
use Illuminate\Http\Response;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Str;
use Laravel\Sanctum\PersonalAccessToken;
use Throwable;

/**
 * Entrar, criar conta e sair, do app. O site tem as telas dele em Livewire.
 */
final class AuthController
{
    /**
     * @throws InvalidCredentialsException
     */
    public function login(LoginRequest $request): AuthTokenResource
    {
        $email = mb_strtolower(mb_trim($request->string('email')->toString()));
        $user = User::query()->where('email', $email)->first();

        throw_if(is_null($user) || is_null($user->password) || ! Hash::check($request->string('password')->toString(), $user->password), InvalidCredentialsException::class);

        $device = $request->string('device')->toString();
        $user->notifyQuietly(new NewLoginNotification('app: '.$device, (string) $request->ip(), (string) $request->userAgent()));

        return new AuthTokenResource($user, $user->createToken($device)->plainTextToken);
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

        return new AuthTokenResource($user, $user->createToken($request->string('device')->toString())->plainTextToken);
    }

    public function logout(Request $request): Response
    {
        $token = $request->user()?->currentAccessToken();

        if ($token instanceof PersonalAccessToken) {
            $token->delete();
        }

        return response()->noContent();
    }
}
