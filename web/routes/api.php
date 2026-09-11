<?php

declare(strict_types=1);

use App\Http\Controllers\Api\Auth\LoginController;
use App\Http\Controllers\Api\Auth\LogoutController;
use App\Http\Controllers\Api\Auth\RegisterController;
use App\Http\Controllers\Api\MeController;
use App\Http\Controllers\Api\ReleaseController;
use Illuminate\Support\Facades\Route;

Route::post('/auth/login', LoginController::class)->middleware('throttle:login')->name('api.auth.login');
Route::post('/auth/register', RegisterController::class)->middleware('throttle:6,1')->name('api.auth.register');

Route::middleware('auth:sanctum')->group(function (): void {
    Route::post('/auth/logout', LogoutController::class)->name('api.auth.logout');
    Route::get('/me', MeController::class)->name('api.me');
});

Route::post('/releases', ReleaseController::class)->middleware('signed.release')->name('api.releases.store');
