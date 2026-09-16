<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Resources\Api\VoiceTokenResource;
use App\Models\Channel;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Request;
use Illuminate\Http\Response;
use Throwable;

/**
 * A voz de um canal: o token de 60 s que o SFU confere, e derrubar alguém de lá.
 */
final class VoiceController
{
    /**
     * @throws Throwable
     */
    public function token(Request $request, Channel $channel, #[CurrentUser] User $user, SfuClient $sfu): VoiceTokenResource
    {
        return new VoiceTokenResource($channel->voiceToken($user, (string) $request->ip(), $request->userAgent(), $sfu));
    }

    /**
     * @throws Throwable
     */
    public function disconnect(Channel $channel, User $user, #[CurrentUser] User $actor, SfuClient $sfu): Response
    {
        $channel->disconnect($actor, $user, $sfu);

        return response()->noContent();
    }
}
