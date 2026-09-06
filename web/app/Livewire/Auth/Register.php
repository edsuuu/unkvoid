<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use Illuminate\Auth\Events\Registered;
use Illuminate\Support\Facades\Auth;
use Illuminate\View\View;
use Laravel\Fortify\Contracts\CreatesNewUsers;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Register')]
final class Register extends Component
{
    public string $name = '';

    public string $email = '';

    public string $password = '';

    public string $password_confirmation = '';

    /**
     * Handle an incoming registration request.
     */
    public function register(CreatesNewUsers $creator): void
    {
        $user = $creator->create([
            'name' => $this->name,
            'email' => $this->email,
            'password' => $this->password,
            'password_confirmation' => $this->password_confirmation,
        ]);

        event(new Registered($user));

        Auth::login($user);

        $this->redirect(route('app', absolute: false), navigate: true);
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.auth.register');
    }
}
