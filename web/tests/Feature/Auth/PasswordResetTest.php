<?php

declare(strict_types=1);

use App\Livewire\Auth\ForgotPassword;
use App\Livewire\Auth\ResetPassword;
use App\Models\User;
use App\Notifications\ResetPasswordNotification;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Password;
use Livewire\Livewire;

it('manda o link de redefinição por e-mail e não revela se a conta existe', function (): void {
    Notification::fake();
    $user = User::factory()->create(['email' => 'edson@unkvoid.test']);

    Livewire::test(ForgotPassword::class)->set('email', 'edson@unkvoid.test')->call('send')->assertSet('sent', true);
    Livewire::test(ForgotPassword::class)->set('email', 'ninguem@unkvoid.test')->call('send')->assertSet('sent', true);

    Notification::assertSentTo($user, ResetPasswordNotification::class);
    Notification::assertCount(1);
});

it('redefine a senha com o token e já entra', function (): void {
    $user = User::factory()->create(['email' => 'edson@unkvoid.test']);
    $token = Password::broker()->createToken($user);

    $this->get(route('password.reset', ['token' => $token, 'email' => 'edson@unkvoid.test']))->assertOk()->assertSee('Nova senha');

    Livewire::test(ResetPassword::class, ['token' => $token, 'email' => 'edson@unkvoid.test'])
        ->set('password', 'senha-nova-123')
        ->set('password_confirmation', 'senha-nova-123')
        ->call('save')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    expect(Hash::check('senha-nova-123', $user->fresh()->password))->toBeTrue();
    $this->assertAuthenticatedAs($user);
});

it('recusa um token inválido', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test']);

    Livewire::test(ResetPassword::class, ['token' => 'lixo', 'email' => 'edson@unkvoid.test'])
        ->set('password', 'senha-nova-123')
        ->set('password_confirmation', 'senha-nova-123')
        ->call('save')
        ->assertHasErrors(['email']);
});
