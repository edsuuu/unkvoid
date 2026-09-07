<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Controllers\Auth\GoogleController;
use App\Http\Controllers\Controller;
use Laravel\Socialite\Facades\Socialite;
use Symfony\Component\HttpFoundation\RedirectResponse as SymfonyRedirect;

/**
 * Entrada do login pelo Google vindo do app.
 *
 * Marca a sessão e manda para o **mesmo** fluxo do navegador. O callback é o do
 * `GoogleController`, já autorizado no Google Cloud: uma URL só, uma configuração a
 * menos para alguém esquecer de cadastrar.
 *
 * O app abre isto no navegador do sistema e recebe o token de volta por deep link
 * (`discord2://auth?token=...`). A senha nunca passa pela janela do app.
 */
final class DesktopAuthController extends Controller
{
    public function redirect(): SymfonyRedirect
    {
        session()->put(GoogleController::DESKTOP, true);

        return Socialite::driver('google')->redirect();
    }
}
