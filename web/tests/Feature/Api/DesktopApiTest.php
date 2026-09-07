<?php

declare(strict_types=1);

use App\Actions\Servers\CreateServer;
use App\Models\Server;
use App\Models\User;

use function Pest\Laravel\getJson;
use function Pest\Laravel\postJson;

/** Creates a user with a known password and returns the desktop app token. */
function tokenFor(User $user): string
{
    return postJson('/api/login', [
        'email' => $user->email,
        'password' => 'senha12345',
        'device' => 'teste',
    ])->json('token');
}

beforeEach(function (): void {
    $this->user = User::factory()->create([
        'email' => 'desktop@teste.local',
        'password' => bcrypt('senha12345'),
        'nickname' => 'desktop',
    ]);
});

it('rejects login with an incorrect password', function (): void {
    postJson('/api/login', [
        'email' => $this->user->email,
        'password' => 'errada',
        'device' => 'teste',
    ])->assertStatus(422);
});

it('returns a token and user on valid login', function (): void {
    $resposta = postJson('/api/login', [
        'email' => $this->user->email,
        'password' => 'senha12345',
        'device' => 'mac',
    ])->assertOk();

    expect($resposta->json('token'))->toBeString()->not->toBeEmpty();
    expect($resposta->json('user.id'))->toBe($this->user->id);
});

it('blocks routes without a token', function (): void {
    getJson('/api/servers')->assertStatus(401);
    getJson('/api/me')->assertStatus(401);
});

it('lists the user’s servers with channels', function (): void {
    app(CreateServer::class)->handle($this->user, 'Meu Servidor');

    $resposta = getJson('/api/servers', ['Authorization' => 'Bearer '.tokenFor($this->user)])->assertOk();

    expect($resposta->json('data.0.name'))->toBe('Meu Servidor');
    expect($resposta->json('data.0.channels'))->toHaveCount(2);
});

it('does not allow viewing a server the user is not a member of', function (): void {
    $outro = User::factory()->create();
    $server = app(CreateServer::class)->handle($outro, 'Alheio');

    getJson("/api/servers/{$server->id}", ['Authorization' => 'Bearer '.tokenFor($this->user)])
        ->assertStatus(403);
});

it('joins a server through an invite', function (): void {
    $dono = User::factory()->create();
    $server = app(CreateServer::class)->handle($dono, 'Com Convite');

    postJson("/api/invites/{$server->invite_code}", [], ['Authorization' => 'Bearer '.tokenFor($this->user)])
        ->assertOk()
        ->assertJsonPath('data.id', $server->id);

    expect(Server::find($server->id)->memberFor($this->user))->not->toBeNull();
});

it('sends and reads a message through the text channel', function (): void {
    $server = app(CreateServer::class)->handle($this->user, 'Com Chat');
    $canal = $server->channels()->where('type', 'text')->first();
    $cabecalho = ['Authorization' => 'Bearer '.tokenFor($this->user)];

    // 201: Laravel detects the newly created model and returns Created in the Resource.
    postJson("/api/channels/{$canal->id}/messages", ['content' => 'olá do desktop'], $cabecalho)
        ->assertCreated()
        ->assertJsonPath('data.content', 'olá do desktop');

    getJson("/api/channels/{$canal->id}/messages", $cabecalho)
        ->assertOk()
        ->assertJsonPath('data.0.content', 'olá do desktop');
});

it('issues a voice token only for voice channels and members', function (): void {
    $server = app(CreateServer::class)->handle($this->user, 'Com Voz');
    $voz = $server->channels()->where('type', 'voice')->first();
    $texto = $server->channels()->where('type', 'text')->first();
    $cabecalho = ['Authorization' => 'Bearer '.tokenFor($this->user)];

    postJson("/api/voice/{$voz->id}/token", [], $cabecalho)->assertOk()->assertJsonStructure(['token', 'room', 'role']);
    postJson("/api/voice/{$texto->id}/token", [], $cabecalho)->assertStatus(403);
});
