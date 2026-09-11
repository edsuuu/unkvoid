<?php

declare(strict_types=1);

use App\Http\Controllers\Api\Auth\LoginController;
use App\Http\Controllers\Api\Auth\LogoutController;
use App\Http\Controllers\Api\Auth\RegisterController;
use App\Http\Controllers\Api\ErrorReportController;
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

// Sem assinatura, de propósito: quem chama é o app instalado na máquina de qualquer
// pessoa, e um segredo dentro do instalador não é segredo. O que protege aqui é o teto
// por IP, o tamanho máximo do log e o fato de a tabela agrupar por erro — encher de lixo
// custa trabalho e não derruba nada.
Route::post('/errors', ErrorReportController::class)->middleware('throttle:30,1')->name('api.errors.store');
