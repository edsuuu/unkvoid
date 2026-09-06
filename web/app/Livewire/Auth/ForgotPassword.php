<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use Illuminate\Support\Facades\Password;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Forgot password')]
final class ForgotPassword extends Component
{
    public string $email = '';

    /**
     * Send a password reset link to the user.
     */
    public function sendPasswordResetLink(): void
    {
        $this->validate([
            'email' => ['required', 'string', 'email'],
        ]);

        $status = Password::broker()->sendResetLink(
            ['email' => $this->email]
        );

        if ($status !== Password::RESET_LINK_SENT) {
            $this->addError('email', __($status));

            return;
        }

        session()->flash('status', __($status));
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.auth.forgot-password');
    }
}
