<?php

declare(strict_types=1);

use App\Http\Controllers\Api\Audits\IndexAuditController;
use App\Http\Controllers\Api\Auth\LoginController;
use App\Http\Controllers\Api\Auth\LogoutController;
use App\Http\Controllers\Api\Auth\RegisterController;
use App\Http\Controllers\Api\Bans\DestroyBanController;
use App\Http\Controllers\Api\Bans\IndexBanController;
use App\Http\Controllers\Api\Bans\StoreBanController;
use App\Http\Controllers\Api\Channels\DestroyChannelController;
use App\Http\Controllers\Api\Channels\StoreChannelController;
use App\Http\Controllers\Api\Channels\UpdateChannelController;
use App\Http\Controllers\Api\Clips\DestroyClipController;
use App\Http\Controllers\Api\Clips\IndexClipController;
use App\Http\Controllers\Api\Clips\PlaylistClipController;
use App\Http\Controllers\Api\Clips\ShowClipController;
use App\Http\Controllers\Api\Clips\StoreClipController;
use App\Http\Controllers\Api\ConfigController;
use App\Http\Controllers\Api\Direct\DestroyDirectMessageController;
use App\Http\Controllers\Api\Direct\IndexConversationController;
use App\Http\Controllers\Api\Direct\MarkReadConversationController;
use App\Http\Controllers\Api\Direct\ShowConversationController;
use App\Http\Controllers\Api\Direct\StoreDirectMessageController;
use App\Http\Controllers\Api\Direct\UpdateDirectMessageController;
use App\Http\Controllers\Api\ErrorReportController;
use App\Http\Controllers\Api\Friends\DestroyFriendController;
use App\Http\Controllers\Api\Friends\IndexFriendController;
use App\Http\Controllers\Api\Friends\StoreFriendController;
use App\Http\Controllers\Api\Friends\UpdateFriendController;
use App\Http\Controllers\Api\Invites\JoinInviteController;
use App\Http\Controllers\Api\MeController;
use App\Http\Controllers\Api\Members\KickMemberController;
use App\Http\Controllers\Api\Members\UpdateMemberController;
use App\Http\Controllers\Api\Messages\DestroyMessageController;
use App\Http\Controllers\Api\Messages\IndexMessageController;
use App\Http\Controllers\Api\Messages\StoreMessageController;
use App\Http\Controllers\Api\Messages\UpdateMessageController;
use App\Http\Controllers\Api\Overwrites\DestroyOverwriteController;
use App\Http\Controllers\Api\Overwrites\PutOverwriteController;
use App\Http\Controllers\Api\ReleaseController;
use App\Http\Controllers\Api\Roles\DestroyRoleController;
use App\Http\Controllers\Api\Roles\StoreRoleController;
use App\Http\Controllers\Api\Roles\UpdateRoleController;
use App\Http\Controllers\Api\Servers\DestroyServerController;
use App\Http\Controllers\Api\Servers\DestroyServerIconController;
use App\Http\Controllers\Api\Servers\IndexServerController;
use App\Http\Controllers\Api\Servers\LeaveServerController;
use App\Http\Controllers\Api\Servers\RegenerateInviteController;
use App\Http\Controllers\Api\Servers\ShowServerController;
use App\Http\Controllers\Api\Servers\StoreServerController;
use App\Http\Controllers\Api\Servers\StoreServerIconController;
use App\Http\Controllers\Api\Servers\UpdateServerController;
use App\Http\Controllers\Api\Sfu\SfuEventController;
use App\Http\Controllers\Api\Voice\DisconnectVoiceController;
use App\Http\Controllers\Api\Voice\VoiceTokenController;
use Illuminate\Support\Facades\Route;

Route::post('/auth/login', LoginController::class)->middleware('throttle:login')->name('api.auth.login');
Route::post('/auth/register', RegisterController::class)->middleware('throttle:6,1')->name('api.auth.register');

Route::get('/config', ConfigController::class)->name('api.config');

Route::middleware('auth:sanctum')->group(function (): void {
    Route::post('/auth/logout', LogoutController::class)->name('api.auth.logout');
    Route::get('/me', MeController::class)->name('api.me');

    Route::get('/servers', IndexServerController::class)->name('api.servers.index');
    Route::post('/servers', StoreServerController::class)->name('api.servers.store');
    Route::get('/servers/{server}', ShowServerController::class)->name('api.servers.show');
    Route::patch('/servers/{server}', UpdateServerController::class)->name('api.servers.update');
    Route::delete('/servers/{server}', DestroyServerController::class)->name('api.servers.destroy');
    Route::post('/servers/{server}/invite', RegenerateInviteController::class)->name('api.servers.invite');
    Route::post('/servers/{server}/leave', LeaveServerController::class)->name('api.servers.leave');
    Route::get('/servers/{server}/audits', IndexAuditController::class)->name('api.audits.index');
    Route::post('/servers/{server}/icon', StoreServerIconController::class)->name('api.servers.icon.store');
    Route::delete('/servers/{server}/icon', DestroyServerIconController::class)->name('api.servers.icon.destroy');
    Route::post('/invites/{code}', JoinInviteController::class)->middleware('throttle:10,1')->name('api.invites.join');

    Route::get('/friends', IndexFriendController::class)->name('api.friends.index');
    Route::post('/friends', StoreFriendController::class)->middleware('throttle:20,1')->name('api.friends.store');
    Route::patch('/friends/{friendship}', UpdateFriendController::class)->name('api.friends.update');
    Route::delete('/friends/{friendship}', DestroyFriendController::class)->name('api.friends.destroy');

    Route::get('/dm', IndexConversationController::class)->name('api.dm.index');
    Route::get('/dm/{user}', ShowConversationController::class)->name('api.dm.show');
    Route::post('/dm/{user}/read', MarkReadConversationController::class)->name('api.dm.read');
    Route::post('/dm/{user}', StoreDirectMessageController::class)->middleware('throttle:60,1')->name('api.dm.store');
    Route::patch('/dm/{directMessage}', UpdateDirectMessageController::class)->name('api.dm.update');
    Route::delete('/dm/{directMessage}', DestroyDirectMessageController::class)->name('api.dm.destroy');

    Route::patch('/servers/{server}/members/{user}', UpdateMemberController::class)->name('api.members.update');
    Route::delete('/servers/{server}/members/{user}', KickMemberController::class)->name('api.members.destroy');
    Route::get('/servers/{server}/bans', IndexBanController::class)->name('api.bans.index');
    Route::post('/servers/{server}/bans/{user}', StoreBanController::class)->name('api.bans.store');
    Route::delete('/servers/{server}/bans/{user}', DestroyBanController::class)->name('api.bans.destroy');

    Route::post('/servers/{server}/roles', StoreRoleController::class)->name('api.roles.store');
    Route::patch('/roles/{role}', UpdateRoleController::class)->name('api.roles.update');
    Route::delete('/roles/{role}', DestroyRoleController::class)->name('api.roles.destroy');

    Route::post('/servers/{server}/channels', StoreChannelController::class)->name('api.channels.store');
    Route::patch('/channels/{channel}', UpdateChannelController::class)->name('api.channels.update');
    Route::delete('/channels/{channel}', DestroyChannelController::class)->name('api.channels.destroy');
    Route::put('/channels/{channel}/overwrites/{type}/{id}', PutOverwriteController::class)->whereNumber('id')->name('api.overwrites.put');
    Route::delete('/channels/{channel}/overwrites/{type}/{id}', DestroyOverwriteController::class)->whereNumber('id')->name('api.overwrites.destroy');

    Route::get('/channels/{channel}/messages', IndexMessageController::class)->name('api.messages.index');
    Route::post('/channels/{channel}/messages', StoreMessageController::class)->middleware('throttle:60,1')->name('api.messages.store');
    Route::patch('/messages/{message}', UpdateMessageController::class)->name('api.messages.update');
    Route::delete('/messages/{message}', DestroyMessageController::class)->name('api.messages.destroy');

    Route::post('/channels/{channel}/voice/token', VoiceTokenController::class)->name('api.voice.token');
    Route::delete('/channels/{channel}/voice/members/{user}', DisconnectVoiceController::class)->name('api.voice.disconnect');

    Route::post('/channels/{channel}/clips', StoreClipController::class)->name('api.clips.store');
    Route::get('/clips', IndexClipController::class)->name('api.clips.index');
    Route::get('/clips/{clip}', ShowClipController::class)->name('api.clips.show');
    Route::delete('/clips/{clip}', DestroyClipController::class)->name('api.clips.destroy');
});

Route::get('/clips/{clip}/playlist.m3u8', PlaylistClipController::class)->middleware('signed')->name('api.clips.playlist');

Route::post('/sfu/events', SfuEventController::class)->middleware('signed.sfu')->name('api.sfu.events');

Route::post('/releases', ReleaseController::class)->middleware('signed.release')->name('api.releases.store');

// Sem assinatura, de propósito: quem chama é o app instalado na máquina de qualquer
// pessoa, e um segredo dentro do instalador não é segredo. O que protege aqui é o teto
// por IP, o tamanho máximo do log e o fato de a tabela agrupar por erro — encher de lixo
// custa trabalho e não derruba nada.
Route::post('/errors', ErrorReportController::class)->middleware('throttle:30,1')->name('api.errors.store');
