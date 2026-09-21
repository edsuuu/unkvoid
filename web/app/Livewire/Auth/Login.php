<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use App\Models\User;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\RateLimiter;
use Illuminate\Validation\ValidationException;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Entrar')]
final class Login extends Component
{
    public string $email = '';

    public string $password = '';

    public bool $remember = true;

    /**
     * @throws ValidationException
     */
    public function login(): void
    {
        $this->validate([
            'email' => ['required', 'string', 'email', 'max:255'],
            'password' => ['required', 'string'],
        ]);

        $key = 'login:'.mb_strtolower(mb_trim($this->email)).'|'.request()->ip();

        if (RateLimiter::tooManyAttempts($key, 5)) {
            throw ValidationException::withMessages([
                'email' => 'Muitas tentativas. Espere um minuto e tente de novo.',
            ]);
        }

        if (! Auth::attempt(['email' => mb_strtolower(mb_trim($this->email)), 'password' => $this->password], $this->remember)) {
            RateLimiter::hit($key);

            throw ValidationException::withMessages([
                'email' => 'E-mail ou senha não conferem.',
            ]);
        }

        RateLimiter::clear($key);
        session()->regenerate();

        /** @var User $user */
        $user = Auth::user();
        $user->notifyNewLoginIfUnknown('site', (string) request()->ip(), (string) request()->userAgent());

        $this->redirectIntended(default: route('home', absolute: false));
    }

    public function render(): View
    {
        return view('livewire.auth.login');
    }
}
