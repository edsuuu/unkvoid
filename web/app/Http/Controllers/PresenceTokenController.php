<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Models\Server;
use App\Support\SfuToken;
use Illuminate\Http\JsonResponse;
use Illuminate\Support\Facades\Auth;
use Symfony\Component\HttpKernel\Exception\AccessDeniedHttpException;

final class PresenceTokenController extends Controller
{
    public function __invoke(Server $server, SfuToken $sfuToken): JsonResponse
    {
        $member = $server->memberFor(Auth::user());

        if (! $member) {
            throw new AccessDeniedHttpException(__('You are not a member of this server.'));
        }

        return response()->json([
            'url' => config('services.sfu.url'),
            'token' => $sfuToken->issuePresence($member),
        ]);
    }
}
