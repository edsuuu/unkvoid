<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use Illuminate\Support\Facades\Password;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Esqueci a senha')]
final class ForgotPassword extends Component
{
    public string $email = '';

    public bool $sent = false;

    public function send(): void
    {
        $this->validate(['email' => ['required', 'string', 'email', 'max:255']]);

        // A resposta é a mesma exista a conta ou não: dizer "esse e-mail não está
        // cadastrado" entregaria de graça a lista de quem tem conta.
        Password::broker()->sendResetLink(['email' => mb_strtolower(mb_trim($this->email))]);

        $this->sent = true;
    }

    public function render(): View
    {
        return view('livewire.auth.forgot-password');
    }
}
