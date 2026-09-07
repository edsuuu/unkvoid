<?php

declare(strict_types=1);

use function Pest\Laravel\get;

/**
 * O fluxo do Google para o app já quebrou uma vez de um jeito que nenhum teste pegava:
 * a rota vivia em `routes/api.php`, que não tem sessão, e o Socialite guarda o `state`
 * do OAuth nela — 500 "Session store not set on request". Este teste bate na rota de
 * verdade, porque é a sessão que falta, não o controller.
 */
it('redirects to Google instead of blowing up for lack of a session', function (): void {
    config()->set('services.google', [
        'client_id' => 'teste',
        'client_secret' => 'teste',
        'redirect' => 'https://exemplo.test/oauth2/google/callback',
    ]);

    $response = get(route('api.desktop.google'));

    $response->assertRedirectContains('accounts.google.com');

    // Sem `state` o fluxo perde a proteção contra CSRF — e é ele que exige sessão.
    expect($response->headers->get('location'))->toContain('state=');
});

/**
 * Um 302 para `discord2://` é bloqueado sem aviso por vários navegadores, e o usuário
 * fica olhando para uma página em branco enquanto o app espera para sempre. O callback
 * devolve uma página com o link clicável — é o clique que faz o navegador perguntar
 * "abrir o Unkvoid?".
 */
it('hands the app back a page with the link, not a blocked redirect', function (): void {
    config()->set('services.google', [
        'client_id' => 'teste',
        'client_secret' => 'teste',
        'redirect' => 'https://exemplo.test/oauth2/google/callback',
    ]);

    // Sem código nem state válidos, o Socialite falha — e o app precisa ser avisado,
    // não ficar esperando um callback que nunca chega.
    $response = get(route('api.desktop.google.callback'));

    $response->assertOk()
        ->assertSee('discord2://auth?erro=', escape: false)
        ->assertDontSee('Location:');
});
