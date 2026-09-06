<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use App\Models\User;
use Illuminate\View\View;
use Laravel\Fortify\Contracts\ResetsUserPasswords;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Reset password')]
final class ResetPassword extends Component
{
    public string $email = '';

    public string $token = '';

    public string $password = '';

    public string $password_confirmation = '';

    /**
     * Mount the component.
     */
    public function mount(string $token, ?string $email = null): void
    {
        $this->token = $token;
        $this->email = $email ?? '';
    }

    /**
     * Handle the password reset request.
     */
    public function resetPassword(ResetsUserPasswords $resetter): void
    {
        $user = User::query()->where('email', $this->email)->first();

        if (! $user) {
            $this->addError('email', __("We can't find a user with that email address."));

            return;
        }

        $resetter->reset($user, [
            'token' => $this->token,
            'email' => $this->email,
            'password' => $this->password,
            'password_confirmation' => $this->password_confirmation,
        ]);

        session()->flash('status', __('Your password has been reset!'));

        $this->redirect(route('login'), navigate: true);
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.auth.reset-password');
    }
}
