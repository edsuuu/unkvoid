<?php

declare(strict_types=1);

use App\Http\Controllers\Auth\AppLoginController;
use App\Http\Controllers\Auth\GoogleController;
use App\Http\Controllers\Auth\LogoutController;
use App\Http\Controllers\DownloadController;
use Illuminate\Support\Facades\Route;

Route::view('/', 'home.index')->name('home');
Route::view('/privacidade', 'legal.privacy')->name('privacy');
Route::view('/termos', 'legal.terms')->name('terms');

Route::prefix('downloads')->name('downloads.')->group(function (): void {
    Route::get('/latest.json', [DownloadController::class, 'manifest'])->name('manifest');
    Route::get('/{slug}', [DownloadController::class, 'platform'])->where('slug', '[a-z-]+')->name('platform');
});

Route::prefix('oauth2')->name('oauth2.')->group(function (): void {
    Route::get('/google', [GoogleController::class, 'redirect'])->name('google');
    Route::get('/google/callback', [GoogleController::class, 'callback'])->name('google.callback');
    Route::get('/app', AppLoginController::class)->name('app');
});

Route::post('/logout', LogoutController::class)->middleware('auth')->name('logout');

Route::middleware('guest')->group(function (): void {
    Route::view('/login', 'auth.login')->name('login');
    Route::view('/cadastro', 'auth.register')->name('register');
    Route::view('/esqueci-a-senha', 'auth.forgot-password')->name('password.request');
    Route::view('/redefinir-senha/{token}', 'auth.reset-password')->name('password.reset');
});

Route::middleware(['auth', 'role:Administrador'])->prefix('admin')->group(function (): void {
    Route::view('/', 'admin.releases.index')->name('admin');

    Route::name('admin.')->group(function (): void {
        Route::view('/erros', 'admin.errors.index')->name('errors');
        Route::view('/auditoria', 'admin.audit.index')->name('audit');
        Route::view('/rede', 'admin.network.index')->name('network');
    });
});
