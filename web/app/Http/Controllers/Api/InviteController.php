<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Servers\StoreInviteRequest;
use App\Http\Resources\Api\ServerInviteResource;
use App\Models\Server;
use App\Models\ServerInvite;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Throwable;

/**
 * Os convites de cada pessoa: listar, criar, revogar, e pausar todos os do servidor.
 */
final class InviteController
{
    public function preview(string $code): JsonResponse
    {
        $invite = ServerInvite::query()->with(['server', 'user'])->where('code', $code)->first();
        $server = $invite?->server;
        if (! $server instanceof Server) {
            $server = Server::query()->where('invite_code', $code)->first();
        }

        abort_if(is_null($server), 404, 'Esse convite não existe. Confira o código.');
        abort_if(! is_null($invite) && $invite->spent(), 404, 'Esse convite venceu. Peça outro.');
        abort_if(! is_null($server->invites_paused_at), 404, 'Os convites deste servidor estão pausados.');

        return response()->json(['data' => [
            'server' => [
                'id' => $server->id,
                'name' => $server->name,
                'icon_url' => $server->iconUrl(),
            ],
            'members' => $server->members()->count(),
            'inviter' => $invite?->user?->name,
        ]]);
    }

    /**
     * @throws Throwable
     */
    public function index(Server $server, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ServerInviteResource::collection($server->invitesFor($user));
    }

    /**
     * @throws Throwable
     */
    public function store(StoreInviteRequest $request, Server $server, #[CurrentUser] User $user): ServerInviteResource
    {
        $channel = $request->string('channel_id')->toString();

        $invite = $server->createInvite($user, $channel === '' ? null : $channel, $request->integer('max_age', 604800), $request->integer('max_uses'));

        return new ServerInviteResource($invite->load(['user', 'channel']));
    }

    /**
     * @throws Throwable
     */
    public function destroy(ServerInvite $invite, #[CurrentUser] User $user): Response
    {
        $invite->server->revokeInvite($user, $invite);

        return response()->noContent();
    }

    /**
     * @throws Throwable
     */
    public function pause(Server $server, #[CurrentUser] User $user): Response
    {
        $server->pauseInvites($user, true);

        return response()->noContent();
    }

    /**
     * @throws Throwable
     */
    public function resume(Server $server, #[CurrentUser] User $user): Response
    {
        $server->pauseInvites($user, false);

        return response()->noContent();
    }
}
