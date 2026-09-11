<?php

declare(strict_types=1);

use App\Models\User;

it('entra pela API e recebe um token', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    $response = $this->postJson('/api/auth/login', [
        'email' => 'edson@unkvoid.test',
        'password' => 'senha-forte-123',
        'device' => 'desktop-linux',
    ]);

    $response->assertOk()->assertJsonStructure(['data' => ['token', 'user' => ['id', 'name', 'email', 'admin']]]);

    $token = $response->json('data.token');

    $this->withToken($token)->getJson('/api/me')->assertOk()->assertJsonPath('data.email', 'edson@unkvoid.test');
});

it('recusa credenciais erradas na API com 401', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    $this->postJson('/api/auth/login', ['email' => 'edson@unkvoid.test', 'password' => 'x', 'device' => 'd'])
        ->assertStatus(401)
        ->assertJsonPath('message', 'E-mail ou senha não conferem.');
});

it('conta só do Google não entra por senha', function (): void {
    User::factory()->create(['email' => 'g@unkvoid.test', 'password' => null, 'google_id' => '123']);

    $this->postJson('/api/auth/login', ['email' => 'g@unkvoid.test', 'password' => 'qualquer', 'device' => 'd'])
        ->assertStatus(401);
});

it('cria a conta pela API e revoga o token no logout', function (): void {
    $response = $this->postJson('/api/auth/register', [
        'name' => 'Edson',
        'email' => 'novo@unkvoid.test',
        'password' => 'senha-forte-123',
        'device' => 'desktop-windows',
    ]);

    $response->assertCreated()->assertJsonPath('data.user.name', 'Edson');

    $token = $response->json('data.token');

    $this->withToken($token)->postJson('/api/auth/logout')->assertNoContent();
    $this->assertDatabaseCount('personal_access_tokens', 0);
});

it('exige token para o /api/me', function (): void {
    $this->getJson('/api/me')->assertUnauthorized();
});
