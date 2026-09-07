<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Actions\Auth\ResolveGoogleUser;
use App\Http\Controllers\Controller;
use Illuminate\Contracts\View\View;
use Illuminate\Support\Facades\Log;
use Laravel\Socialite\Facades\Socialite;
use Symfony\Component\HttpFoundation\RedirectResponse as SymfonyRedirect;
use Throwable;

/**
 * Desktop app login through Google.
 *
 * The app has no browser session, so the flow is: open the system browser,
 * the user signs in with Google, and the callback returns the token via deep link
 * (`discord2://auth?token=...`). The token never passes through the app window.
 *
 * O callback devolve uma **página**, não um 302 para o deep link: redirecionamento
 * automático para esquema externo é bloqueado sem aviso por vários navegadores. Quem
 * dispara o link é um clique da pessoa, e é aí que o navegador pergunta "abrir o
 * Unkvoid?" — a permissão de que o fluxo depende.
 */
final class DesktopAuthController extends Controller
{
    public function redirect(): SymfonyRedirect
    {
        return Socialite::driver('google')
            ->redirectUrl(route('api.desktop.google.callback'))
            ->redirect();
    }

    public function callback(ResolveGoogleUser $resolveGoogleUser): View
    {
        try {
            $googleUser = Socialite::driver('google')
                ->redirectUrl(route('api.desktop.google.callback'))
                ->user();
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] desktop Google login failed', ['exception' => $exception]);

            $mensagem = __('Unable to sign in with Google.');

            return view('desktop-handoff', [
                'erro' => $mensagem,
                'link' => 'discord2://auth?erro='.urlencode($mensagem),
            ]);
        }

        $user = $resolveGoogleUser->handle($googleUser);
        $token = $user->createToken('desktop-google')->plainTextToken;

        return view('desktop-handoff', [
            'erro' => null,
            'link' => 'discord2://auth?token='.urlencode($token),
        ]);
    }
}
