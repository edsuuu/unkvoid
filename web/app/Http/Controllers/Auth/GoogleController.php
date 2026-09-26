<?php

declare(strict_types=1);

namespace App\Http\Controllers\Auth;

use App\Models\User;
use App\Notifications\WelcomeNotification;
use App\Services\Auth\AppTokens;
use Illuminate\Http\RedirectResponse;
use Illuminate\Http\Request;
use Illuminate\Http\Response;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Laravel\Socialite\Facades\Socialite;
use Symfony\Component\HttpFoundation\RedirectResponse as SocialiteRedirect;
use Throwable;

/**
 * Entrar pelo Google: a ida e a volta.
 */
final readonly class GoogleController
{
    public function __construct(
        private AppTokens $tokens,
    ) {}

    public function redirect(): SocialiteRedirect
    {
        return Socialite::driver('google')->redirect();
    }

    public function callback(Request $request): RedirectResponse|Response
    {
        try {
            $googleUser = Socialite::driver('google')->user();
        } catch (Throwable $exception) {
            Log::channel('daily')->warning('[WARN] o Google não devolveu o usuário', [
                'message' => $exception->getMessage(),
            ]);

            return to_route('login')->with('status', 'Não deu para entrar com o Google. Tente de novo.');
        }

        $email = mb_strtolower(mb_trim((string) $googleUser->getEmail()));

        if ($email === '') {
            return to_route('login')->with('status', 'A conta do Google não tem e-mail.');
        }

        try {
            $user = User::query()
                ->where('google_id', (string) $googleUser->getId())
                ->orWhere('email', $email)
                ->first();

            $created = is_null($user);

            if (is_null($user)) {
                $user = User::query()->create([
                    'name' => User::freeNickname(mb_trim((string) $googleUser->getName()) ?: $email),
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
            $user->notifyNewLoginIfUnknown('Google', (string) $request->ip(), (string) $request->userAgent());
        }

        // O app abriu o navegador com uma porta local esperando o token: em vez de sessão,
        // ele recebe um token do Sanctum e o `state` de volta, e a página some. Ver AppLoginController.
        $port = $request->session()->pull('app_port');
        $state = $request->session()->pull('app_state');
        $renewable = $request->session()->pull('app_refresh') === true;

        if (is_string($state)) {
            $tokens = $this->tokens->issue($user, 'app', $renewable);
            $token = $tokens['token'];
            $refresh = is_null($tokens['refresh_token']) ? '' : '&refresh_token='.urlencode($tokens['refresh_token']);

            // O app até a 0.0.28 espera numa porta local; do 0.0.29 em diante ele registra
            // o esquema `unkvoid://` e não manda porta. Atender os dois é o que impede o
            // login de quebrar para quem ainda não atualizou.
            if (is_int($port)) {
                return redirect()->away("http://127.0.0.1:{$port}/?token=".urlencode($token).$refresh.'&state='.$state);
            }

            return response()->view('auth.app-return', [
                'link' => 'unkvoid://login?token='.urlencode($token).$refresh.'&state='.$state,
                'name' => $user->name,
            ]);
        }

        Auth::login($user, true);
        $request->session()->regenerate();

        return redirect()->intended(route('home', absolute: false));
    }
}
