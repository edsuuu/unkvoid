<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use App\Models\User;
use Illuminate\Support\Facades\Auth;
use Illuminate\Validation\ValidationException;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Confirm password')]
final class ConfirmPassword extends Component
{
    public string $password = '';

    /**
     * Confirm the user's password.
     */
    public function confirmPassword(): void
    {
        $this->validate([
            'password' => ['required', 'string'],
        ]);

        /** @var User $user */
        $user = Auth::user();

        if (! Auth::guard('web')->validate([
            'email' => $user->email,
            'password' => $this->password,
        ])) {
            throw ValidationException::withMessages([
                'password' => __('auth.password'),
            ]);
        }

        session()->put('auth.password_confirmed_at', time());

        $this->redirectIntended(default: route('app', absolute: false), navigate: true);
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.auth.confirm-password');
    }
}
