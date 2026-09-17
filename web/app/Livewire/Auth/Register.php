<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use App\Models\User;
use App\Notifications\WelcomeNotification;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Illuminate\Validation\Rules\Password;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;
use Throwable;

#[Title('Criar conta')]
final class Register extends Component
{
    public string $name = '';

    public string $email = '';

    public string $password = '';

    public string $password_confirmation = '';

    /**
     * @throws Throwable
     */
    public function register(): void
    {
        /** @var array{name: string, email: string, password: string} $validated */
        $validated = $this->validate([
            'name' => ['required', 'string', 'min:3', 'max:32', 'regex:/^[A-Za-z0-9._]+$/', 'unique:users,name'],
            'email' => ['required', 'string', 'email', 'max:255', 'unique:users,email'],
            'password' => ['required', 'string', 'confirmed', Password::min(8)],
        ], [
            'name.regex' => 'O apelido aceita letras, números, ponto e _ — sem espaço.',
            'name.unique' => 'Esse apelido já é de outra pessoa.',
        ]);

        try {
            $user = User::query()->create([
                'name' => mb_trim($validated['name']),
                'email' => mb_strtolower(mb_trim($validated['email'])),
                'password' => $validated['password'],
                'nickname_confirmed_at' => now(),
            ]);
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao criar a conta', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'email' => $validated['email'],
            ]);

            throw $exception;
        }

        Auth::login($user, true);
        session()->regenerate();
        $user->notifyQuietly(new WelcomeNotification);

        $this->redirect(route('home', absolute: false));
    }

    public function render(): View
    {
        return view('livewire.auth.register');
    }
}
