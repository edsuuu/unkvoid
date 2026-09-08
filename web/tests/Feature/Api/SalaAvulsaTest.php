<?php

declare(strict_types=1);

use function Pest\Laravel\postJson;

/**
 * A sala avulsa é o app inteiro: sem ela não há como entrar. E como ela é anônima, o
 * código é a única chave — por isso o formato é conferido aqui, e não só na leitura.
 */
it('cria uma sala e devolve um código sorteado', function (): void {
    $resposta = postJson('/api/rooms', ['name' => 'Edsu']);

    $resposta->assertOk()->assertJsonStructure(['room', 'url', 'token']);

    expect($resposta->json('room'))->toMatch('/^[a-f0-9]{12}$/');
});

it('entra numa sala existente sem inventar outra', function (): void {
    $codigo = postJson('/api/rooms', ['name' => 'Edsu'])->json('room');

    expect(postJson('/api/rooms', ['name' => 'Amigo', 'room' => $codigo])->json('room'))
        ->toBe($codigo);
});

it('sorteia um código diferente a cada sala', function (): void {
    $primeira = postJson('/api/rooms', ['name' => 'Edsu'])->json('room');
    $segunda = postJson('/api/rooms', ['name' => 'Edsu'])->json('room');

    expect($primeira)->not->toBe($segunda);
});

/**
 * Aceitar um nome escolhido à mão devolveria a sala "jogatina" para qualquer um que
 * digitasse "jogatina" — que é exatamente o que o código sorteado existe para evitar.
 */
it('recusa código fora do formato sorteado', function (string $codigo): void {
    postJson('/api/rooms', ['name' => 'Edsu', 'room' => $codigo])->assertInvalid('room');
})->with(['jogatina', 'ABCDEF012345', 'abc', 'a1b2c3d4e5f6g7']);

it('exige um nome', function (): void {
    postJson('/api/rooms', [])->assertInvalid('name');
});

/** Dois participantes na mesma sala não podem receber o mesmo id, ou um derruba o outro. */
it('dá um participante diferente para cada entrada', function (): void {
    $codigo = postJson('/api/rooms', ['name' => 'Edsu'])->json('room');

    $sub = fn (string $token): string => json_decode(
        base64_decode(strtr(explode('.', $token)[1], '-_', '+/')),
        true,
    )['sub'];

    $um = $sub(postJson('/api/rooms', ['name' => 'Edsu', 'room' => $codigo])->json('token'));
    $outro = $sub(postJson('/api/rooms', ['name' => 'Edsu', 'room' => $codigo])->json('token'));

    expect($um)->not->toBe($outro);
});
