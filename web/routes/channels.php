<?php

declare(strict_types=1);

use App\Enums\PermissionEnum;
use App\Models\Channel;
use App\Models\Server;
use App\Models\User;
use Illuminate\Support\Facades\Broadcast;

Broadcast::channel('channel.{channel}', function (User $user, Channel $channel): bool {
    $member = $channel->server->memberOf($user);

    return ! is_null($member) && $member->can(PermissionEnum::ViewChannel, $channel);
});

Broadcast::channel('server.{server}', function (User $user, Server $server): array|false {
    if (is_null($server->memberOf($user))) {
        return false;
    }

    return ['id' => $user->id, 'name' => $user->name, 'avatar_url' => $user->avatar_url];
});

Broadcast::channel('user.{id}', fn (User $user, int $id): bool => $user->id === $id);
