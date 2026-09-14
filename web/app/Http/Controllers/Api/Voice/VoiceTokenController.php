<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Voice;

use App\Http\Resources\Api\VoiceTokenResource;
use App\Models\Channel;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Request;
use Throwable;

final class VoiceTokenController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Request $request, Channel $channel, #[CurrentUser] User $user, SfuClient $sfu): VoiceTokenResource
    {
        return new VoiceTokenResource($channel->voiceToken($user, (string) $request->ip(), $request->userAgent(), $sfu));
    }
}
