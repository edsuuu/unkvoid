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
            throw new AccessDeniedHttpException(__('Este canal não é de voz.'));
        }

        $member = $channel->server->memberFor(Auth::user());

        if (! $member) {
            throw new AccessDeniedHttpException(__('Você não é membro deste servidor.'));
        }

        return response()->json([
            'url' => config('services.sfu.url'),
            'room' => $channel->id,
            'role' => $member->role,
            'token' => $sfuToken->issue($member, $channel),
        ]);
    }
}
