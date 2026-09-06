<?php

declare(strict_types=1);

namespace App\Actions\Servers;

use App\Models\Server;
use App\Models\ServerMember;
use App\Models\User;
use Illuminate\Support\Facades\Log;
use Throwable;

final class JoinServerByInvite
{
    /**
     * @throws Throwable
     */
    public function handle(User $user, string $inviteCode): Server
    {
        $server = Server::where('invite_code', $inviteCode)->firstOrFail();

        try {
            ServerMember::firstOrCreate(
                ['server_id' => $server->id, 'user_id' => $user->id],
                ['role' => 'member'],
            );
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERRO] falha ao entrar no servidor', [
                'user' => $user->id,
                'server' => $server->id,
                'exception' => $exception,
            ]);

            throw $exception;
        }

        return $server;
    }
}
