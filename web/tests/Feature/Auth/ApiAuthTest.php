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
        'email' => 'novo@unkvoid.test',
        'password' => 'senha-forte-123',
        'device' => 'desktop-windows',
    ]);

    $response->assertCreated()->assertJsonPath('data.user.name', 'novo');

    $token = $response->json('data.token');

    $this->withToken($token)->postJson('/api/auth/logout')->assertNoContent();
    $this->assertDatabaseCount('personal_access_tokens', 0);
});

it('exige token para o /api/me', function (): void {
    $this->getJson('/api/me')->assertUnauthorized();
});

it('o cadastro pela API tira o apelido do e-mail, sem confirmar, e desempata quando já existe', function (): void {
    User::factory()->create(['name' => 'edson.lima']);

    $this->postJson('/api/auth/register', ['email' => 'Edson.Lima@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])
        ->assertCreated()
        ->assertJsonPath('data.user.name', 'edson.lima2')
        ->assertJsonPath('data.user.nickname_confirmed', false);

    expect(User::query()->where('email', 'edson.lima@unkvoid.test')->firstOrFail()->nickname_confirmed_at)->toBeNull();
});

it('o /api/me diz se o apelido já foi confirmado', function (): void {
    $this->actingAs(User::factory()->create())->getJson('/api/me')->assertOk()->assertJsonPath('data.nickname_confirmed', true);
    $this->actingAs(User::factory()->unconfirmedNickname()->create())->getJson('/api/me')->assertOk()->assertJsonPath('data.nickname_confirmed', false);
});

it('escolhe o apelido pelo PATCH /api/me e confirma', function (): void {
    $user = User::factory()->unconfirmedNickname()->create(['name' => 'novo']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'edsu.dev'])
        ->assertOk()
        ->assertJsonPath('data.name', 'edsu.dev')
        ->assertJsonPath('data.nickname_confirmed', true);

    expect($user->fresh()?->hasConfirmedNickname())->toBeTrue();
});

it('ficar com o apelido automático também confirma', function (): void {
    $user = User::factory()->unconfirmedNickname()->create(['name' => 'novo']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'novo'])
        ->assertOk()
        ->assertJsonPath('data.name', 'novo')
        ->assertJsonPath('data.nickname_confirmed', true);
});

it('não troca o apelido de quem já confirmou', function (): void {
    $user = User::factory()->create(['name' => 'edsu']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'outro.nome'])
        ->assertForbidden()
        ->assertJsonPath('message', 'Você já escolheu o seu apelido.');

    expect($user->fresh()?->name)->toBe('edsu');
});

it('o PATCH /api/me exige token', function (): void {
    $this->patchJson('/api/me', ['name' => 'edsu'])->assertUnauthorized();
});
