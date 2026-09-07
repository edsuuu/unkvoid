<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Actions\Auth\ResolveGoogleUser;
use App\Http\Controllers\Controller;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Log;
use Laravel\Socialite\Facades\Socialite;
use Symfony\Component\HttpFoundation\RedirectResponse as SymfonyRedirect;
use Throwable;

/**
 * Login do app desktop pelo Google.
 *
 * O app não tem sessão de navegador, então o fluxo é: abre o navegador do sistema,
 * a pessoa entra no Google, e o callback devolve o token por deep link
 * (`discord2://auth?token=...`). O token nunca passa pela janela do app.
 */
final class DesktopAuthController extends Controller
{
    public function redirect(): SymfonyRedirect
    {
        return Socialite::driver('google')
            ->redirectUrl(route('api.desktop.google.callback'))
            ->redirect();
    }

    public function callback(ResolveGoogleUser $resolveGoogleUser): RedirectResponse
    {
        try {
            $googleUser = Socialite::driver('google')
                ->redirectUrl(route('api.desktop.google.callback'))
                ->user();
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERRO] login do desktop pelo Google falhou', ['exception' => $exception]);

            return redirect()->away('discord2://auth?erro='.urlencode(__('Não foi possível entrar com o Google.')));
        }

        $user = $resolveGoogleUser->handle($googleUser);
        $token = $user->createToken('desktop-google')->plainTextToken;

        return redirect()->away('discord2://auth?token='.urlencode($token));
    }
}
