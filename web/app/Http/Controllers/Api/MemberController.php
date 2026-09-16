<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Servers\UpdateMemberRequest;
use App\Http\Resources\Api\MemberResource;
use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

/**
 * Membro de um servidor: apelido, cargos, mudo, surdo — e expulsar.
 */
final class MemberController
{
    /**
     * @throws Throwable
     */
    public function update(UpdateMemberRequest $request, Server $server, User $user, #[CurrentUser] User $actor, SfuClient $sfu): MemberResource
    {
        /** @var array{nickname?: ?string, role_ids?: array<int, int>, server_mute?: bool, server_deaf?: bool} $changes */
        $changes = $request->validated();

        return new MemberResource($server->updateMember($actor, $user, $changes, $sfu)->load(['user', 'roles']));
    }

    /**
     * @throws Throwable
     */
    public function destroy(Server $server, User $user, #[CurrentUser] User $actor, SfuClient $sfu): Response
    {
        $server->kick($actor, $user, $sfu);

        return response()->noContent();
    }
}
