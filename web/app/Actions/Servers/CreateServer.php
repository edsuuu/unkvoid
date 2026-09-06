<?php

declare(strict_types=1);

namespace App\Actions\Servers;

use App\Models\Channel;
use App\Models\Server;
use App\Models\ServerMember;
use App\Models\User;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Throwable;

final class CreateServer
{
    /**
     * @throws Throwable
     */
    public function handle(User $owner, string $name): Server
    {
        try {
            return DB::transaction(function () use ($owner, $name): Server {
                $server = Server::create([
                    'owner_id' => $owner->id,
                    'name' => $name,
                    'invite_code' => Server::generateInviteCode(),
                ]);

                ServerMember::create([
                    'server_id' => $server->id,
                    'user_id' => $owner->id,
                    'role' => 'owner',
                ]);

                Channel::create(['server_id' => $server->id, 'name' => 'geral', 'type' => 'text', 'position' => 0]);
                Channel::create(['server_id' => $server->id, 'name' => 'Geral', 'type' => 'voice', 'position' => 0]);

                return $server;
            });
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERRO] falha ao criar servidor', [
                'owner' => $owner->id,
                'exception' => $exception,
            ]);

            throw $exception;
        }
    }
}
