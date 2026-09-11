<?php

declare(strict_types=1);

use App\Models\User;
use Laravel\Socialite\Facades\Socialite;
use Laravel\Socialite\Two\User as GoogleUser;

function googleUser(string $id, string $email, string $name): GoogleUser
{
    $user = new GoogleUser;
    $user->map(['id' => $id, 'email' => $email, 'name' => $name, 'avatar' => 'https://lh3.example/foto.png']);

    return $user;
}

it('redireciona para o Google', function (): void {
    $this->get(route('oauth2.google'))->assertRedirect();
});

it('cria a conta na primeira entrada com o Google', function (): void {
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-1', 'Edson@Unkvoid.test', 'Edson'));

    $this->get(route('oauth2.google.callback'))->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticated();
    $this->assertDatabaseHas('users', ['email' => 'edson@unkvoid.test', 'google_id' => 'g-1']);
});

it('vincula o Google a uma conta que já existia pelo e-mail', function (): void {
    $existing = User::factory()->create(['email' => 'edson@unkvoid.test']);
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-2', 'edson@unkvoid.test', 'Edson'));

    $this->get(route('oauth2.google.callback'));

    $this->assertAuthenticatedAs($existing);
    expect($existing->fresh()->google_id)->toBe('g-2');
    $this->assertDatabaseCount('users', 1);
});

it('devolve o token para o app pela porta local', function (): void {
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-3', 'app@unkvoid.test', 'App'));

    $this->get(route('oauth2.app', ['port' => 43123]))->assertRedirect(route('oauth2.google'));

    $response = $this->get(route('oauth2.google.callback'));

    $response->assertRedirect();
    expect($response->headers->get('Location'))->toStartWith('http://127.0.0.1:43123/?token=');
    $this->assertDatabaseCount('personal_access_tokens', 1);
});

it('quem entra com o e-mail do dono vira administrador', function (): void {
    $this->seed(Database\Seeders\Seeder001Roles::class);
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-4', config('unkvoid.admin_email'), 'Dono'));

    $this->get(route('oauth2.google.callback'));

    expect(User::query()->firstOrFail()->isAdmin())->toBeTrue();
});
