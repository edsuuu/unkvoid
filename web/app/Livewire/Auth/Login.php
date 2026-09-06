<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use Illuminate\Support\Facades\Auth;
use Illuminate\Validation\ValidationException;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Log in')]
final class Login extends Component
{
    public string $email = '';

    public string $password = '';

    public bool $remember = false;

    /**
     * Handle the authentication attempt.
     */
    public function login(): void
    {
        $this->validate([
            'email' => ['required', 'string', 'email'],
            'password' => ['required', 'string'],
        ]);

        if (! Auth::attempt(['email' => $this->email, 'password' => $this->password], $this->remember)) {
            throw ValidationException::withMessages([
                'email' => __('auth.failed'),
            ]);
        }

        session()->regenerate();

        $this->redirectIntended(default: route('app', absolute: false), navigate: true);
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.auth.login');
    }
}
