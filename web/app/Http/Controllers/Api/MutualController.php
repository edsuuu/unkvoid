<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Enums\FriendshipStatusEnum;
use App\Models\Friendship;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\JsonResponse;

final class MutualController
{
    public function show(User $user, #[CurrentUser] User $viewer): JsonResponse
    {
        $sharedServers = Server::query()
            ->join('server_members as viewer_members', 'viewer_members.server_id', '=', 'servers.id')
            ->join('server_members as target_members', 'target_members.server_id', '=', 'servers.id')
            ->where('viewer_members.user_id', $viewer->id)
            ->where('target_members.user_id', $user->id)
            ->select('servers.*')
            ->get();

        $friendship = Friendship::between($viewer, $user);
        $canSee = $friendship?->status !== FriendshipStatusEnum::Blocked
            && ($friendship?->status === FriendshipStatusEnum::Accepted || $sharedServers->isNotEmpty());
        abort_unless($canSee, 404, 'Não foi possível encontrar essa pessoa.');

        $viewerFriends = Friendship::query()
            ->where('status', FriendshipStatusEnum::Accepted->value)
            ->where(fn ($query) => $query->where('requester_id', $viewer->id)->orWhere('addressee_id', $viewer->id))
            ->get()
            ->map(fn (Friendship $friendship): int => $friendship->requester_id === $viewer->id ? $friendship->addressee_id : $friendship->requester_id);
        $targetFriends = Friendship::query()
            ->where('status', FriendshipStatusEnum::Accepted->value)
            ->where(fn ($query) => $query->where('requester_id', $user->id)->orWhere('addressee_id', $user->id))
            ->get()
            ->map(fn (Friendship $friendship): int => $friendship->requester_id === $user->id ? $friendship->addressee_id : $friendship->requester_id);

        $mutualFriendCount = $viewerFriends->intersect($targetFriends)->reject(fn (int $id): bool => in_array($id, [$viewer->id, $user->id], true))->count();

        return response()->json([
            'mutual_friends' => $mutualFriendCount,
            'mutual_servers' => $sharedServers->map(fn (Server $server): array => [
                'id' => $server->id,
                'name' => $server->name,
                'icon_url' => $server->iconUrl(),
            ])->values(),
        ]);
    }
}
