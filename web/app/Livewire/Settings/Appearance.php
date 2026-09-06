<?php

declare(strict_types=1);

namespace App\Livewire\Settings;

use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Appearance settings')]
final class Appearance extends Component
{
    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View
    {
        return view('livewire.settings.appearance');
    }
}
