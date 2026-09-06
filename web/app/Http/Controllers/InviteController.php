<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Actions\Servers\JoinServerByInvite;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Throwable;

final class InviteController extends Controller
{
    public function __invoke(string $code, JoinServerByInvite $joinServerByInvite): RedirectResponse
    {
        try {
            $server = $joinServerByInvite->handle(Auth::user(), $code);
        } catch (Throwable $exception) {
            Log::channel('servers')->warning('[WARN] convite inválido', ['code' => $code, 'exception' => $exception]);

            return redirect()->route('app')->withErrors(['invite' => __('Convite inválido ou expirado.')]);
        }

        $channel = $server->channels()->where('type', 'text')->first();

        return redirect()->route('channel', ['server' => $server->id, 'channel' => $channel?->id]);
    }
}
