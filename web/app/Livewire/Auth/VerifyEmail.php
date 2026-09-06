<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use App\Models\User;
use Illuminate\Support\Facades\Auth;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Verify email')]
final class VerifyEmail extends Component
{
    /**
     * Send an email verification notification to the user.
     */
    public function sendVerification(): void
    {
        /** @var User $user */
        $user = Auth::user();

        if ($user->hasVerifiedEmail()) {
            $this->redirectIntended(default: route('app', absolute: false), navigate: true);

            return;
        }

        $user->sendEmailVerificationNotification();

        session()->flash('status', 'verification-link-sent');
    }

    /**
     * Log the current user out of the application.
     */
    public function logout(): void
    {
        Auth::guard('web')->logout();

        session()->invalidate();
        session()->regenerateToken();

        $this->redirect(route('login'), navigate: true);
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.auth.verify-email');
    }
}
