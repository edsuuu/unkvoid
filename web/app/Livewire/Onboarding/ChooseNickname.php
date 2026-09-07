<?php

declare(strict_types=1);

namespace App\Livewire\Onboarding;

use Illuminate\Contracts\View\View;
use Illuminate\Support\Facades\Auth;
use Illuminate\Validation\Rule;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Choose your nickname')]
final class ChooseNickname extends Component
{
    public string $name = '';

    public string $nickname = '';

    public function mount(): void
    {
        $user = Auth::user();

        $this->name = $user->name;
        $this->nickname = mb_strtolower(preg_replace('/[^a-zA-Z0-9]/', '', $user->name) ?? '');
    }

    public function save(): void
    {
        $user = Auth::user();

        $validated = $this->validate([
            'name' => ['required', 'string', 'min:2', 'max:60'],
            'nickname' => [
                'required', 'string', 'min:3', 'max:24', 'regex:/^[a-z0-9_.]+$/',
                Rule::unique('users', 'nickname')->ignore($user->id),
            ],
        ], [
            'nickname.regex' => __('Use only lowercase letters, numbers, periods, and underscores.'),
        ]);

        $user->fill($validated)->save();

        $this->redirectRoute('app', navigate: true);
    }

    public function render(): View
    {
        return view('livewire.onboarding.choose-nickname');
    }
}
