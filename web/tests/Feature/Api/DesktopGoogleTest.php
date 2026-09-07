<?php

declare(strict_types=1);

use App\Http\Controllers\Auth\GoogleController;

use function Pest\Laravel\get;
use function Pest\Laravel\withSession;

beforeEach(function (): void {
    config()->set('services.google', [
        'client_id' => 'teste',
        'client_secret' => 'teste',
        'redirect' => 'https://exemplo.test/oauth2/google/callback',
    ]);
});

/**
 * Já quebrou uma vez de um jeito que nenhum teste pegava: a rota vivia em
 * `routes/api.php`, que não tem sessão, e o Socialite guarda o `state` do OAuth nela —
 * 500 "Session store not set on request".
 */
it('redirects to Google instead of blowing up for lack of a session', function (): void {
    $response = get(route('api.desktop.google'));

    $response->assertRedirectContains('accounts.google.com');

    // Sem `state` o fluxo perde a proteção contra CSRF — e é ele que exige sessão.
    expect($response->headers->get('location'))->toContain('state=');
});

/**
 * O app usa o MESMO callback do navegador. Um caminho separado exigiria uma segunda URL
 * autorizada no Google Cloud, e enquanto ela não fosse cadastrada o Google recusaria com
 * `redirect_uri_mismatch` — que foi exatamente o que aconteceu.
 */
it('sends the app through the same callback the browser already uses', function (): void {
    $doApp = get(route('api.desktop.google'))->headers->get('location');
    $doNavegador = get(route('google.redirect'))->headers->get('location');

    $uri = fn (string $url): string => parse_url_query($url)['redirect_uri'];

    // O mesmo redirect_uri nos dois: uma URL só para autorizar no Google Cloud.
    expect($uri($doApp))->toBe($uri($doNavegador));

    // E a marca que faz o callback devolver o deep link em vez de logar no navegador.
    get(route('api.desktop.google'));
    expect(session(GoogleController::DESKTOP))->toBeTrue();
});

function parse_url_query(string $url): array
{
    parse_str(parse_url($url, PHP_URL_QUERY) ?? '', $query);

    return $query;
}

/**
 * Um 302 para `discord2://` é bloqueado sem aviso por vários navegadores, e o usuário
 * fica olhando uma página em branco enquanto o app espera para sempre. O callback
 * devolve uma página com o link clicável — é o clique que faz o navegador perguntar
 * "abrir o Unkvoid?".
 */
it('hands the app back a page with the link, not a blocked redirect', function (): void {
    $response = withSession([GoogleController::DESKTOP => true])->get(route('google.callback'));

    $response->assertOk()->assertSee('discord2://auth?erro=', escape: false);
});

/** Sem a marca na sessão, o mesmo callback continua sendo o login normal do navegador. */
it('keeps the browser flow untouched', function (): void {
    get(route('google.callback'))->assertRedirect(route('login'));
});
