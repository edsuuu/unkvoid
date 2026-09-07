<?php

declare(strict_types=1);

namespace App\Http\Controllers\Auth;

use App\Actions\Auth\ResolveGoogleUser;
use App\Http\Controllers\Controller;
use Illuminate\Contracts\View\View;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Laravel\Socialite\Facades\Socialite;
use Symfony\Component\HttpFoundation\RedirectResponse as SymfonyRedirect;
use Throwable;

/**
 * Login pelo Google, para o navegador **e** para o app.
 *
 * Os dois passam por este mesmo callback de propósito. Um caminho separado para o app
 * exigiria uma segunda URL autorizada no Google Cloud, e enquanto ela não fosse
 * cadastrada o Google recusaria com `redirect_uri_mismatch` — que foi exatamente o que
 * aconteceu. Uma URL só é uma configuração a menos para alguém esquecer.
 *
 * Quem distingue os dois é a sessão, marcada antes do redirecionamento.
 */
final class GoogleController extends Controller
{
    /** Marca na sessão que este fluxo veio do app, e não do navegador. */
    public const DESKTOP = 'oauth.desktop';

    public function redirect(): SymfonyRedirect
    {
        session()->forget(self::DESKTOP);

        return Socialite::driver('google')->redirect();
    }

    public function callback(ResolveGoogleUser $resolveGoogleUser): RedirectResponse|View
    {
        $doApp = session()->pull(self::DESKTOP, false);

        try {
            $googleUser = Socialite::driver('google')->user();
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] Google callback failed', ['exception' => $exception]);

            $mensagem = __('Unable to sign in with Google.');

            if ($doApp) {
                return view('desktop-handoff', [
                    'erro' => $mensagem,
                    'link' => 'discord2://auth?erro='.urlencode($mensagem),
                ]);
            }

            return redirect()->route('login')->withErrors(['email' => $mensagem]);
        }

        $user = $resolveGoogleUser->handle($googleUser);

        if ($doApp) {
            // O app não tem sessão de navegador: ele recebe um token de API pelo deep
            // link. A senha e o cookie nunca passam pela janela do app.
            return view('desktop-handoff', [
                'erro' => null,
                'link' => 'discord2://auth?token='.urlencode($user->createToken('desktop-google')->plainTextToken),
            ]);
        }

        Auth::login($user, remember: true);

        return redirect()->intended(route('app'));
    }
}
