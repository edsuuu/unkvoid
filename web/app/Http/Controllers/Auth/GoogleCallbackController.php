<?php

declare(strict_types=1);

namespace App\Http\Controllers\Auth;

use App\Models\User;
use App\Notifications\NewLoginNotification;
use App\Notifications\WelcomeNotification;
use Illuminate\Http\RedirectResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Laravel\Socialite\Facades\Socialite;
use Throwable;

final class GoogleCallbackController
{
    public function __invoke(Request $request): RedirectResponse
    {
        try {
            $googleUser = Socialite::driver('google')->user();
        } catch (Throwable $exception) {
            Log::channel('daily')->warning('[WARN] o Google não devolveu o usuário', [
                'message' => $exception->getMessage(),
            ]);

            return redirect()->route('login')->with('status', 'Não deu para entrar com o Google. Tente de novo.');
        }

        $email = mb_strtolower(mb_trim((string) $googleUser->getEmail()));

        if ($email === '') {
            return redirect()->route('login')->with('status', 'A conta do Google não tem e-mail.');
        }

        try {
            $user = User::query()
                ->where('google_id', (string) $googleUser->getId())
                ->orWhere('email', $email)
                ->first();

            $created = is_null($user);

            if (is_null($user)) {
                $user = User::query()->create([
                    'name' => mb_trim((string) $googleUser->getName()) ?: $email,
                    'email' => $email,
                    'google_id' => (string) $googleUser->getId(),
                    'avatar_url' => $googleUser->getAvatar(),
                    'email_verified_at' => now(),
                ]);
            } else {
                $user->forceFill([
                    'google_id' => (string) $googleUser->getId(),
                    'avatar_url' => $googleUser->getAvatar() ?? $user->avatar_url,
                    'email_verified_at' => $user->email_verified_at ?? now(),
                ])->save();
            }
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao gravar a conta vinda do Google', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'email' => $email,
            ]);

            throw $exception;
        }

        if ($created) {
            $user->notifyQuietly(new WelcomeNotification);
        } else {
            $user->notifyQuietly(new NewLoginNotification('Google', (string) $request->ip(), (string) $request->userAgent()));
        }

        // O app abriu o navegador com uma porta local esperando o token: em vez de sessão,
        // ele recebe um token do Sanctum e a página some. Ver AppLoginController.
        $port = $request->session()->pull('app_port');

        if (is_int($port)) {
            $token = $user->createToken('app')->plainTextToken;

            return redirect()->away("http://127.0.0.1:{$port}/?token=".urlencode($token));
        }

        Auth::login($user, true);
        $request->session()->regenerate();

        return redirect()->intended(route('home', absolute: false));
    }
}
