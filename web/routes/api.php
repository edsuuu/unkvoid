<?php

declare(strict_types=1);

use App\Http\Controllers\Api\AuthController;
use App\Http\Controllers\Api\DesktopAuthController;
use App\Http\Controllers\Api\MessageController;
use App\Http\Controllers\Api\ServerController;
use App\Http\Controllers\PresenceTokenController;
use App\Http\Controllers\VoiceTokenController;
use Illuminate\Support\Facades\Route;

Route::post('login', [AuthController::class, 'login'])->middleware('throttle:6,1')->name('api.login');

// Google for the desktop app: it goes through the system browser and returns via deep link.
Route::get('desktop/google', [DesktopAuthController::class, 'redirect'])->name('api.desktop.google');
Route::get('desktop/google/callback', [DesktopAuthController::class, 'callback'])->name('api.desktop.google.callback');

Route::middleware('auth:sanctum')->group(function (): void {
    Route::get('me', [AuthController::class, 'me'])->name('api.me');
    Route::post('logout', [AuthController::class, 'logout'])->name('api.logout');

    Route::get('servers', [ServerController::class, 'index'])->name('api.servers.index');
    Route::post('servers', [ServerController::class, 'store'])->name('api.servers.store');
    Route::get('servers/{server}', [ServerController::class, 'show'])->name('api.servers.show');
    Route::delete('servers/{server}', [ServerController::class, 'destroy'])->name('api.servers.destroy');
    Route::post('invites/{code}', [ServerController::class, 'join'])->name('api.invites.join');

    Route::get('channels/{channel}/messages', [MessageController::class, 'index'])->name('api.messages.index');
    Route::post('channels/{channel}/messages', [MessageController::class, 'store'])->name('api.messages.store');

    // Same issuers as the web app: the SFU token does not change for desktop.
    Route::post('voice/{channel}/token', VoiceTokenController::class)->name('api.voice.token');
    Route::post('servers/{server}/presence', PresenceTokenController::class)->name('api.presence.token');
});
