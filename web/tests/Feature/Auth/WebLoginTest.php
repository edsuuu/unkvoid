<?php

declare(strict_types=1);

use App\Livewire\Auth\Login;
use App\Livewire\Auth\Register;
use App\Models\User;
use Database\Seeders\Seeder001Roles;
use Livewire\Livewire;

it('abre as telas de entrar e de criar conta', function (): void {
    $this->get(route('login'))->assertOk()->assertSee('Entrar com Google');
    $this->get(route('register'))->assertOk()->assertSee('Criar conta com Google');
});

it('entra com e-mail e senha pelo site', function (): void {
    $user = User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)
        ->set('email', 'Edson@unkvoid.test')
        ->set('password', 'senha-forte-123')
        ->call('login')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticatedAs($user);
});

it('recusa senha errada sem dizer qual campo errou', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)
        ->set('email', 'edson@unkvoid.test')
        ->set('password', 'outra')
        ->call('login')
        ->assertHasErrors(['email']);

    $this->assertGuest();
});

it('cria a conta pelo site e já entra', function (): void {
    Livewire::test(Register::class)
        ->set('name', 'Edson')
        ->set('email', 'novo@unkvoid.test')
        ->set('password', 'senha-forte-123')
        ->set('password_confirmation', 'senha-forte-123')
        ->call('register')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticated();
    $this->assertDatabaseHas('users', ['email' => 'novo@unkvoid.test']);
    expect(User::query()->where('email', 'novo@unkvoid.test')->firstOrFail()->hasConfirmedNickname())->toBeTrue();
});

it('sai da conta', function (): void {
    $user = User::factory()->create();

    $this->actingAs($user)->post(route('logout'))->assertRedirect(route('home'));
    $this->assertGuest();
});

it('só o e-mail do dono nasce administrador', function (): void {
    $this->seed(Seeder001Roles::class);

    $common = User::factory()->create();
    $this->actingAs($common, 'sanctum')->getJson('/api/me')->assertOk()->assertJsonPath('data.admin', false);

    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);
    $this->actingAs($admin, 'sanctum')->getJson('/api/me')->assertOk()->assertJsonPath('data.admin', true);
});
