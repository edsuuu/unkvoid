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

Route::get('/downloads/latest.json', [DownloadController::class, 'manifest'])->name('downloads.manifest');
Route::get('/downloads/{slug}', [DownloadController::class, 'platform'])->where('slug', '[a-z-]+')->name('downloads.platform');

Route::middleware('guest')->group(function (): void {
    Route::view('/login', 'auth.login')->name('login');
    Route::view('/cadastro', 'auth.register')->name('register');
    Route::view('/esqueci-a-senha', 'auth.forgot-password')->name('password.request');
    Route::view('/redefinir-senha/{token}', 'auth.reset-password')->name('password.reset');
});

Route::get('/oauth2/google', [GoogleController::class, 'redirect'])->name('oauth2.google');
Route::get('/oauth2/google/callback', [GoogleController::class, 'callback'])->name('oauth2.google.callback');
Route::get('/oauth2/app', AppLoginController::class)->name('oauth2.app');
Route::post('/logout', LogoutController::class)->middleware('auth')->name('logout');

Route::middleware(['auth', 'role:Administrador'])->group(function (): void {
    Route::view('/admin', 'admin.releases.index')->name('admin');
    Route::view('/admin/erros', 'admin.errors.index')->name('admin.errors');
    Route::view('/admin/auditoria', 'admin.audit.index')->name('admin.audit');
    Route::view('/admin/rede', 'admin.network.index')->name('admin.network');
});
