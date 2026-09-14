<?php

declare(strict_types=1);

namespace App\Http\Controllers\Auth;

use App\Http\Requests\Auth\AppLoginRequest;
use Illuminate\Http\RedirectResponse;

/**
 * O app não consegue fazer o login do Google dentro do próprio webview (o Google recusa),
 * então abre o navegador do sistema aqui, com um servidor local esperando na porta que
 * informa. O callback do Google devolve o token para essa porta, junto com o `state` que
 * o app mandou: sem ele igual, o app recusa o que chegar.
 */
final class AppLoginController
{
    public function __invoke(AppLoginRequest $request): RedirectResponse
    {
        $request->session()->put('app_port', $request->integer('port'));
        $request->session()->put('app_state', $request->string('state')->toString());

        return to_route('oauth2.google');
    }
}
