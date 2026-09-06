<?php

declare(strict_types=1);

namespace App\Livewire\Settings;

use App\Concerns\PasswordValidationRules;
use App\Livewire\Actions\Logout;
use App\Models\User;
use Illuminate\Support\Facades\Auth;
use Illuminate\View\View;
use Livewire\Component;

final class DeleteUserForm extends Component
{
    use PasswordValidationRules;

    public string $password = '';

    /**
     * Delete the currently authenticated user.
     */
    public function deleteUser(Logout $logout): void
    {
        $this->validate([
            'password' => $this->currentPasswordRules(),
        ]);

        /** @var User $user */
        $user = Auth::user();

        tap($user, $logout(...))->delete();

        $this->redirect('/', navigate: true);
    }

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.settings.delete-user-form');
    }
}
