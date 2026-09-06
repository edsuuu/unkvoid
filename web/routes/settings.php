<?php

declare(strict_types=1);

use Illuminate\Support\Facades\Route;
use Laravel\Fortify\Features;

Route::middleware(['auth'])->group(function (): void {
    Route::redirect('settings', 'settings/profile');

    Route::view('settings/profile', 'settings.profile')->name('profile.edit');
});

Route::middleware(['auth', 'verified'])->group(function (): void {
    Route::view('settings/appearance', 'settings.appearance')->name('appearance.edit');

    Route::view('settings/security', 'settings.security')
        ->middleware(
            when(
                Features::canManageTwoFactorAuthentication()
                && Features::optionEnabled(Features::twoFactorAuthentication(), 'confirmPassword'),
                ['password.confirm'],
                [],
            ),
        )
        ->name('security.edit');
});
