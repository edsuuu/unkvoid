<?php

declare(strict_types=1);

namespace App\Livewire\Settings\TwoFactor;

use App\Models\User;
use Exception;
use Illuminate\View\View;
use Laravel\Fortify\Actions\GenerateNewRecoveryCodes;
use Livewire\Attributes\Locked;
use Livewire\Component;

final class RecoveryCodes extends Component
{
    /**
     * @var array<int, string>
     */
    #[Locked]
    public array $recoveryCodes = [];

    /**
     * Mount the component.
     */
    public function mount(): void
    {
        $this->loadRecoveryCodes();
    }

    /**
     * Generate new recovery codes for the user.
     */
    public function regenerateRecoveryCodes(GenerateNewRecoveryCodes $generateNewRecoveryCodes): void
    {
        /** @var User $user */
        $user = auth()->user();

        $generateNewRecoveryCodes($user);

        $this->loadRecoveryCodes();
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.settings.two-factor.recovery-codes');
    }

    /**
     * Load the recovery codes for the user.
     */
    private function loadRecoveryCodes(): void
    {
        /** @var User $user */
        $user = auth()->user();

        if ($user->hasEnabledTwoFactorAuthentication() && $user->two_factor_recovery_codes) {
            try {
                $decrypted = decrypt($user->two_factor_recovery_codes);
                $codes = is_string($decrypted) ? json_decode($decrypted, true) : [];
                $this->recoveryCodes = is_array($codes) ? array_values(array_filter($codes, is_string(...))) : [];
            } catch (Exception) {
                $this->addError('recoveryCodes', 'Failed to load recovery codes');

                $this->recoveryCodes = [];
            }
        }
    }
}
