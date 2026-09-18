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
use App\Http\Controllers\Api\RoomController;
use App\Http\Controllers\Api\ServerController;
use App\Http\Controllers\Api\Sfu\SfuEventController;
use App\Http\Controllers\Api\VoiceController;
use Illuminate\Support\Facades\Route;

Route::name('api.')->group(function (): void {
    Route::prefix('auth')->name('auth.')->group(function (): void {
        Route::post('/login', [AuthController::class, 'login'])->middleware('throttle:login')->name('login');
        Route::post('/register', [AuthController::class, 'register'])->middleware('throttle:6,1')->name('register');
        Route::post('/logout', [AuthController::class, 'logout'])->middleware('auth:sanctum')->name('logout');
    });

    Route::get('/config', ConfigController::class)->name('config');

    // A playlist é pedida pelo tocador de vídeo, que não manda o token do app: quem
    // autoriza aqui é a URL assinada, e por isso ela fica fora do `auth:sanctum`.
    Route::get('/clips/{clip}/playlist.m3u8', [ClipController::class, 'playlist'])->middleware('signed')->name('clips.playlist');

    Route::post('/sfu/events', SfuEventController::class)->middleware('signed.sfu')->name('sfu.events');
    Route::post('/releases', ReleaseController::class)->middleware('signed.release')->name('releases.store');

    // Sem assinatura, de propósito: quem chama é o app instalado na máquina de qualquer
    // pessoa, e um segredo dentro do instalador não é segredo. O que protege aqui é o teto
    // por IP, o tamanho máximo do log e o fato de a tabela agrupar por erro — encher de lixo
    // custa trabalho e não derruba nada.
    Route::post('/errors', ErrorReportController::class)->middleware('throttle:30,1')->name('errors.store');

    Route::middleware('auth:sanctum')->group(function (): void {
        Route::get('/me', [MeController::class, 'show'])->name('me');
        Route::patch('/me', [MeController::class, 'update'])->middleware('throttle:20,1')->name('me.update');
        Route::post('/me/avatar', [MeController::class, 'storeAvatar'])->middleware('throttle:20,1')->name('me.avatar.store');
        Route::delete('/me/avatar', [MeController::class, 'destroyAvatar'])->name('me.avatar.destroy');

        Route::post('/rooms/{code}/token', [RoomController::class, 'token'])
            ->where('code', '[a-z0-9][a-z0-9-]{1,30}[a-z0-9]')
            ->middleware('throttle:30,1')
            ->name('rooms.token');

        Route::prefix('servers')->group(function (): void {
            Route::get('/', [ServerController::class, 'index'])->name('servers.index');
            Route::post('/', [ServerController::class, 'store'])->name('servers.store');

            Route::prefix('{server}')->group(function (): void {
                Route::get('/', [ServerController::class, 'show'])->name('servers.show');
                Route::patch('/', [ServerController::class, 'update'])->name('servers.update');
                Route::delete('/', [ServerController::class, 'destroy'])->name('servers.destroy');
                Route::post('/invite', [ServerController::class, 'regenerateInvite'])->name('servers.invite');
                Route::post('/leave', [ServerController::class, 'leave'])->name('servers.leave');
                Route::post('/icon', [ServerController::class, 'storeIcon'])->name('servers.icon.store');
                Route::delete('/icon', [ServerController::class, 'destroyIcon'])->name('servers.icon.destroy');

                // Auditoria, cargos, canais, membros e banidos pendem do servidor no
                // caminho, mas são recursos próprios: o nome da rota é o deles.
                Route::get('/audits', [ServerController::class, 'audits'])->name('audits.index');
                Route::post('/roles', [RoleController::class, 'store'])->name('roles.store');
                Route::post('/channels', [ChannelController::class, 'store'])->name('channels.store');

                Route::prefix('members')->name('members.')->group(function (): void {
                    Route::patch('/{user}', [MemberController::class, 'update'])->name('update');
                    Route::delete('/{user}', [MemberController::class, 'destroy'])->name('destroy');
                });

                Route::prefix('bans')->name('bans.')->group(function (): void {
                    Route::get('/', [BanController::class, 'index'])->name('index');
                    Route::post('/{user}', [BanController::class, 'store'])->name('store');
                    Route::delete('/{user}', [BanController::class, 'destroy'])->name('destroy');
                });
            });
        });

        Route::post('/invites/{code}', [ServerController::class, 'join'])->middleware('throttle:10,1')->name('invites.join');

        Route::prefix('roles/{role}')->name('roles.')->group(function (): void {
            Route::patch('/', [RoleController::class, 'update'])->name('update');
            Route::delete('/', [RoleController::class, 'destroy'])->name('destroy');
        });

        Route::prefix('channels/{channel}')->group(function (): void {
            Route::patch('/', [ChannelController::class, 'update'])->name('channels.update');
            Route::delete('/', [ChannelController::class, 'destroy'])->name('channels.destroy');

            Route::prefix('overwrites/{type}/{id}')->name('overwrites.')->group(function (): void {
                Route::put('/', [OverwriteController::class, 'put'])->whereNumber('id')->name('put');
                Route::delete('/', [OverwriteController::class, 'destroy'])->whereNumber('id')->name('destroy');
            });

            Route::prefix('messages')->name('messages.')->group(function (): void {
                Route::get('/', [MessageController::class, 'index'])->name('index');
                Route::post('/', [MessageController::class, 'store'])->middleware('throttle:60,1')->name('store');
            });

            Route::prefix('voice')->name('voice.')->group(function (): void {
                Route::post('/token', [VoiceController::class, 'token'])->name('token');
                Route::delete('/members/{user}', [VoiceController::class, 'disconnect'])->name('disconnect');
            });

            Route::post('/clips', [ClipController::class, 'store'])->name('clips.store');
        });

        Route::prefix('messages/{message}')->name('messages.')->group(function (): void {
            Route::patch('/', [MessageController::class, 'update'])->name('update');
            Route::delete('/', [MessageController::class, 'destroy'])->name('destroy');
        });

        Route::prefix('friends')->name('friends.')->group(function (): void {
            Route::get('/', [FriendController::class, 'index'])->name('index');
            Route::post('/', [FriendController::class, 'store'])->middleware('throttle:20,1')->name('store');
            Route::patch('/{friendship}', [FriendController::class, 'update'])->name('update');
            Route::delete('/{friendship}', [FriendController::class, 'destroy'])->name('destroy');
        });

        Route::prefix('dm')->name('dm.')->group(function (): void {
            Route::get('/', [DirectMessageController::class, 'index'])->name('index');
            Route::get('/{user}', [DirectMessageController::class, 'show'])->name('show');
            Route::post('/{user}/read', [DirectMessageController::class, 'read'])->name('read');
            Route::post('/{user}', [DirectMessageController::class, 'store'])->middleware('throttle:60,1')->name('store');
            Route::patch('/{directMessage}', [DirectMessageController::class, 'update'])->name('update');
            Route::delete('/{directMessage}', [DirectMessageController::class, 'destroy'])->name('destroy');
        });

        Route::prefix('clips')->name('clips.')->group(function (): void {
            Route::get('/', [ClipController::class, 'index'])->name('index');
            Route::get('/{clip}', [ClipController::class, 'show'])->name('show');
            Route::delete('/{clip}', [ClipController::class, 'destroy'])->name('destroy');
        });
    });
});
