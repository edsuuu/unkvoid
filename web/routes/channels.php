<?php

use App\Models\Channel;
use App\Models\User;
use Illuminate\Support\Facades\Broadcast;

Broadcast::channel('App.Models.User.{id}', function ($user, $id) {
    return (int) $user->id === (int) $id;
});

/**
 * Only someone who is a member of the server owning the channel may listen. This is the
 * same rule the component applies when reading, in one place: the socket must not be a
 * side door into a server the person was removed from.
 */
Broadcast::channel('channel.{channelId}', function (User $user, string $channelId): bool {
    $channel = Channel::find($channelId);

    return $channel !== null && $channel->server->members()->where('user_id', $user->id)->exists();
});
