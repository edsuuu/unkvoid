<?php

declare(strict_types=1);

use App\Http\Controllers\Auth\GoogleController;
use App\Http\Controllers\InviteController;
use App\Http\Controllers\VoiceTokenController;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Route;

// A raiz é a porta de entrada: quem está logado vai pro workspace, o resto loga.
Route::get('/', fn () => redirect()->route(Auth::check() ? 'app' : 'login'))->name('home');

Route::get('oauth2/google', [GoogleController::class, 'redirect'])->name('google.redirect');
Route::get('oauth2/google/callback', [GoogleController::class, 'callback'])->name('google.callback');

Route::middleware(['auth'])->group(function (): void {
    Route::view('bem-vindo', 'onboarding')->name('onboarding');
});

Route::middleware(['auth', 'nickname'])->group(function (): void {
    Route::view('canais', 'app')->name('app');
    Route::view('canais/{server}/{channel?}', 'app')->name('channel');

    Route::get('convite/{code}', InviteController::class)->name('invite');
    Route::post('api/voz/{channel}/token', VoiceTokenController::class)->name('voice.token');
});

require __DIR__.'/settings.php';
