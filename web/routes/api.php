<?php

declare(strict_types=1);

use App\Http\Controllers\Api\AuthController;
use App\Http\Controllers\Api\BanController;
use App\Http\Controllers\Api\ChannelController;
use App\Http\Controllers\Api\ClipController;
use App\Http\Controllers\Api\ConfigController;
use App\Http\Controllers\Api\DirectMessageController;
use App\Http\Controllers\Api\ErrorReportController;
use App\Http\Controllers\Api\FriendController;
use App\Http\Controllers\Api\MeController;
use App\Http\Controllers\Api\MemberController;
use App\Http\Controllers\Api\MessageController;
use App\Http\Controllers\Api\OverwriteController;
use App\Http\Controllers\Api\ReleaseController;
use App\Http\Controllers\Api\RoleController;
use App\Http\Controllers\Api\ServerController;
use App\Http\Controllers\Api\Sfu\SfuEventController;
use App\Http\Controllers\Api\VoiceController;
use Illuminate\Support\Facades\Route;

Route::post('/auth/login', [AuthController::class, 'login'])->middleware('throttle:login')->name('api.auth.login');
Route::post('/auth/register', [AuthController::class, 'register'])->middleware('throttle:6,1')->name('api.auth.register');

Route::get('/config', ConfigController::class)->name('api.config');

Route::middleware('auth:sanctum')->group(function (): void {
    Route::post('/auth/logout', [AuthController::class, 'logout'])->name('api.auth.logout');
    Route::get('/me', MeController::class)->name('api.me');

    Route::get('/servers', [ServerController::class, 'index'])->name('api.servers.index');
    Route::post('/servers', [ServerController::class, 'store'])->name('api.servers.store');
    Route::get('/servers/{server}', [ServerController::class, 'show'])->name('api.servers.show');
    Route::patch('/servers/{server}', [ServerController::class, 'update'])->name('api.servers.update');
    Route::delete('/servers/{server}', [ServerController::class, 'destroy'])->name('api.servers.destroy');
    Route::post('/servers/{server}/invite', [ServerController::class, 'regenerateInvite'])->name('api.servers.invite');
    Route::post('/servers/{server}/leave', [ServerController::class, 'leave'])->name('api.servers.leave');
    Route::get('/servers/{server}/audits', [ServerController::class, 'audits'])->name('api.audits.index');
    Route::post('/servers/{server}/icon', [ServerController::class, 'storeIcon'])->name('api.servers.icon.store');
    Route::delete('/servers/{server}/icon', [ServerController::class, 'destroyIcon'])->name('api.servers.icon.destroy');
    Route::post('/invites/{code}', [ServerController::class, 'join'])->middleware('throttle:10,1')->name('api.invites.join');

    Route::get('/friends', [FriendController::class, 'index'])->name('api.friends.index');
    Route::post('/friends', [FriendController::class, 'store'])->middleware('throttle:20,1')->name('api.friends.store');
    Route::patch('/friends/{friendship}', [FriendController::class, 'update'])->name('api.friends.update');
    Route::delete('/friends/{friendship}', [FriendController::class, 'destroy'])->name('api.friends.destroy');

    Route::get('/dm', [DirectMessageController::class, 'index'])->name('api.dm.index');
    Route::get('/dm/{user}', [DirectMessageController::class, 'show'])->name('api.dm.show');
    Route::post('/dm/{user}/read', [DirectMessageController::class, 'read'])->name('api.dm.read');
    Route::post('/dm/{user}', [DirectMessageController::class, 'store'])->middleware('throttle:60,1')->name('api.dm.store');
    Route::patch('/dm/{directMessage}', [DirectMessageController::class, 'update'])->name('api.dm.update');
    Route::delete('/dm/{directMessage}', [DirectMessageController::class, 'destroy'])->name('api.dm.destroy');

    Route::patch('/servers/{server}/members/{user}', [MemberController::class, 'update'])->name('api.members.update');
    Route::delete('/servers/{server}/members/{user}', [MemberController::class, 'destroy'])->name('api.members.destroy');
    Route::get('/servers/{server}/bans', [BanController::class, 'index'])->name('api.bans.index');
    Route::post('/servers/{server}/bans/{user}', [BanController::class, 'store'])->name('api.bans.store');
    Route::delete('/servers/{server}/bans/{user}', [BanController::class, 'destroy'])->name('api.bans.destroy');

    Route::post('/servers/{server}/roles', [RoleController::class, 'store'])->name('api.roles.store');
    Route::patch('/roles/{role}', [RoleController::class, 'update'])->name('api.roles.update');
    Route::delete('/roles/{role}', [RoleController::class, 'destroy'])->name('api.roles.destroy');

    Route::post('/servers/{server}/channels', [ChannelController::class, 'store'])->name('api.channels.store');
    Route::patch('/channels/{channel}', [ChannelController::class, 'update'])->name('api.channels.update');
    Route::delete('/channels/{channel}', [ChannelController::class, 'destroy'])->name('api.channels.destroy');
    Route::put('/channels/{channel}/overwrites/{type}/{id}', [OverwriteController::class, 'put'])->whereNumber('id')->name('api.overwrites.put');
    Route::delete('/channels/{channel}/overwrites/{type}/{id}', [OverwriteController::class, 'destroy'])->whereNumber('id')->name('api.overwrites.destroy');

    Route::get('/channels/{channel}/messages', [MessageController::class, 'index'])->name('api.messages.index');
    Route::post('/channels/{channel}/messages', [MessageController::class, 'store'])->middleware('throttle:60,1')->name('api.messages.store');
    Route::patch('/messages/{message}', [MessageController::class, 'update'])->name('api.messages.update');
    Route::delete('/messages/{message}', [MessageController::class, 'destroy'])->name('api.messages.destroy');

    Route::post('/channels/{channel}/voice/token', [VoiceController::class, 'token'])->name('api.voice.token');
    Route::delete('/channels/{channel}/voice/members/{user}', [VoiceController::class, 'disconnect'])->name('api.voice.disconnect');

    Route::post('/channels/{channel}/clips', [ClipController::class, 'store'])->name('api.clips.store');
    Route::get('/clips', [ClipController::class, 'index'])->name('api.clips.index');
    Route::get('/clips/{clip}', [ClipController::class, 'show'])->name('api.clips.show');
    Route::delete('/clips/{clip}', [ClipController::class, 'destroy'])->name('api.clips.destroy');
});

Route::get('/clips/{clip}/playlist.m3u8', [ClipController::class, 'playlist'])->middleware('signed')->name('api.clips.playlist');

Route::post('/sfu/events', SfuEventController::class)->middleware('signed.sfu')->name('api.sfu.events');

Route::post('/releases', ReleaseController::class)->middleware('signed.release')->name('api.releases.store');

// Sem assinatura, de propósito: quem chama é o app instalado na máquina de qualquer
// pessoa, e um segredo dentro do instalador não é segredo. O que protege aqui é o teto
// por IP, o tamanho máximo do log e o fato de a tabela agrupar por erro — encher de lixo
// custa trabalho e não derruba nada.
Route::post('/errors', ErrorReportController::class)->middleware('throttle:30,1')->name('api.errors.store');
