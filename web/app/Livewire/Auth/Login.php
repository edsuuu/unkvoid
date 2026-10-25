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
        // `email` é o e-mail ou o apelido (`User::loginColumn`).
        $this->validate([
            'email' => ['required', 'string', 'max:255'],
            'password' => ['required', 'string'],
        ]);

        $login = mb_strtolower(mb_trim($this->email));
        $key = 'login:'.$login.'|'.request()->ip();

        if (RateLimiter::tooManyAttempts($key, 5)) {
            throw ValidationException::withMessages([
                'email' => 'Muitas tentativas. Espere um minuto e tente de novo.',
            ]);
        }

        if (! Auth::attempt([User::loginColumn($login) => $login, 'password' => $this->password], $this->remember)) {
            RateLimiter::hit($key);

            throw ValidationException::withMessages([
                'email' => 'E-mail, apelido ou senha não conferem.',
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
