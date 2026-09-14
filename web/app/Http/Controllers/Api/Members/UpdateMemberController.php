<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Members;

use App\Http\Requests\Api\Servers\UpdateMemberRequest;
use App\Http\Resources\Api\MemberResource;
use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class UpdateMemberController
{
    /**
     * @throws Throwable
     */
    public function __invoke(UpdateMemberRequest $request, Server $server, User $user, #[CurrentUser] User $actor, SfuClient $sfu): MemberResource
    {
        /** @var array{nickname?: ?string, role_ids?: array<int, int>, server_mute?: bool, server_deaf?: bool} $changes */
        $changes = $request->validated();

        return new MemberResource($server->updateMember($actor, $user, $changes, $sfu)->load(['user', 'roles']));
    }
}
