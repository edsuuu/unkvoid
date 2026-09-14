<?php

declare(strict_types=1);

use App\Livewire\Auth\Login;
use App\Livewire\Auth\Register;
use App\Models\User;
use App\Notifications\NewLoginNotification;
use App\Notifications\ResetPasswordNotification;
use App\Notifications\WelcomeNotification;
use Illuminate\Support\Facades\Notification;
use Laravel\Socialite\Facades\Socialite;
use Laravel\Socialite\Two\User as GoogleUser;
use Livewire\Livewire;

it('manda boas-vindas ao criar a conta pelo site e pela API', function (): void {
    Notification::fake();

    Livewire::test(Register::class)
        ->set('name', 'Edson')->set('email', 'site@unkvoid.test')
        ->set('password', 'senha-forte-123')->set('password_confirmation', 'senha-forte-123')
        ->call('register');

    $this->postJson('/api/auth/register', ['name' => 'Edson', 'email' => 'api@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])->assertCreated();

    Notification::assertSentTo(User::query()->where('email', 'site@unkvoid.test')->firstOrFail(), WelcomeNotification::class);
    Notification::assertSentTo(User::query()->where('email', 'api@unkvoid.test')->firstOrFail(), WelcomeNotification::class);
});

it('avisa de um novo acesso pelo site, pela API e pelo Google', function (): void {
    Notification::fake();
    $user = User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)->set('email', 'edson@unkvoid.test')->set('password', 'senha-forte-123')->call('login');
    $this->postJson('/api/auth/login', ['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'notebook'])->assertOk();

    $googleUser = new GoogleUser;
    $googleUser->map(['id' => 'g-9', 'email' => 'edson@unkvoid.test', 'name' => 'Edson', 'avatar' => null]);
    Socialite::shouldReceive('driver->user')->andReturn($googleUser);
    $this->get(route('oauth2.google.callback'));

    Notification::assertSentToTimes($user, NewLoginNotification::class, 3);
});

it('renderiza os três e-mails em HTML com o desenho do site', function (): void {
    $user = User::factory()->create(['name' => 'Edson', 'email' => 'edson@unkvoid.test']);

    $welcome = (new WelcomeNotification)->toMail($user)->render();
    expect((string) $welcome)->toContain('Bem-vindo, Edson.')->toContain('<table')->toContain('unkvoid-mark.png')->toContain('#06050a');

    $login = new NewLoginNotification('site', '203.0.113.9', 'Firefox')->toMail($user)->render();
    expect((string) $login)->toContain('Novo acesso')->toContain('203.0.113.9')->toContain(route('password.request'));

    $reset = new ResetPasswordNotification('abc')->toMail($user)->render();
    expect((string) $reset)->toContain('Redefinir')->toContain(route('password.reset', ['token' => 'abc', 'email' => 'edson@unkvoid.test']));
});
