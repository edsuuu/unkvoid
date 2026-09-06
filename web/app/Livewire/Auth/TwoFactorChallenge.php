<?php

declare(strict_types=1);

namespace App\Livewire\Auth;

use App\Models\User;
use Illuminate\Support\Facades\Auth;
use Illuminate\View\View;
use Laravel\Fortify\Events\TwoFactorAuthenticationFailed;
use Laravel\Fortify\Http\Requests\TwoFactorLoginRequest;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Two-factor challenge')]
final class TwoFactorChallenge extends Component
{
    /**
     * The recovery code.
     */
    public string $recovery_code = '';

    /**
     * The two-factor authentication code.
     */
    public string $code = '';

    /**
     * Log in using the two-factor authentication code.
     */
    public function login(): void
    {
        $this->validate([
            'code' => ['nullable', 'string'],
            'recovery_code' => ['nullable', 'string'],
        ]);

        if ($this->recovery_code !== '' && $this->recovery_code !== '0') {
            $this->loginWithRecoveryCode();
        } else {
            $this->loginWithTwoFactorCode();
        }
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.auth.two-factor-challenge');
    }

    /**
     * Log in using the two-factor authentication code.
     */
    private function loginWithTwoFactorCode(): void
    {
        $request = resolve(TwoFactorLoginRequest::class);

        $request->merge(['code' => $this->code]);

        /** @var User $user */
        $user = $request->challengedUser();

        if ($request->hasValidCode()) {
            Auth::login($user, $request->remember());

            $request->session()->forget('login.id');

            $this->redirectIntended(default: route('app', absolute: false), navigate: true);
        } else {
            event(new TwoFactorAuthenticationFailed($user));

            $this->addError('code', __('The provided two-factor authentication code was invalid.'));
        }
    }

    /**
     * Log in using the recovery code.
     */
    private function loginWithRecoveryCode(): void
    {
        $request = resolve(TwoFactorLoginRequest::class);

        $request->merge(['recovery_code' => $this->recovery_code]);

        /** @var User $user */
        $user = $request->challengedUser();

        if ($recoveryCode = $request->validRecoveryCode()) {
            $user->replaceRecoveryCode($recoveryCode);

            Auth::login($user, $request->remember());

            $request->session()->forget('login.id');

            $this->redirectIntended(default: route('app', absolute: false), navigate: true);
        } else {
            event(new TwoFactorAuthenticationFailed($user));

            $this->addError('recovery_code', __('The provided recovery code was invalid.'));
        }
    }
}
