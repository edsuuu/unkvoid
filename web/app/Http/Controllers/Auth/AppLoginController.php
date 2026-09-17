<?php

declare(strict_types=1);

namespace App\Http\Controllers\Auth;

use App\Http\Requests\Auth\AppLoginRequest;
use Illuminate\Http\RedirectResponse;

/**
 * O app não consegue fazer o login do Google dentro do próprio webview (o Google recusa),
 * então abre o navegador do sistema aqui. O callback do Google devolve o token pelo
 * `unkvoid://` (ou pela porta local, no app até a 0.0.28), junto com o `state` que o app
 * mandou: sem ele igual, o app recusa o que chegar.
 */
final class AppLoginController
{
    public function __invoke(AppLoginRequest $request): RedirectResponse
    {
        // Sem porta tem de ficar nulo: o `integer()` devolveria 0, e o callback mandaria o
        // app novo para `127.0.0.1:0` em vez de abrir o `unkvoid://`.
        $request->session()->put('app_port', $request->filled('port') ? $request->integer('port') : null);
        $request->session()->put('app_state', $request->string('state')->toString());

        return to_route('oauth2.google');
    }
}
