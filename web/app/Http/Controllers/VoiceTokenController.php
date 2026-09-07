<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Models\Channel;
use App\Support\SfuToken;
use Illuminate\Http\JsonResponse;
use Illuminate\Support\Facades\Auth;
use Symfony\Component\HttpKernel\Exception\AccessDeniedHttpException;

final class VoiceTokenController extends Controller
{
    public function __invoke(Channel $channel, SfuToken $sfuToken): JsonResponse
    {
        if (! $channel->isVoice()) {
            throw new AccessDeniedHttpException(__('This is not a voice channel.'));
        }

        $member = $channel->server->memberFor(Auth::user());

        if (! $member) {
            throw new AccessDeniedHttpException(__('You are not a member of this server.'));
        }

        return response()->json([
            'url' => config('services.sfu.url'),
            'room' => $channel->id,
            'role' => $member->role,
            'token' => $sfuToken->issue($member, $channel),
        ]);
    }
}
