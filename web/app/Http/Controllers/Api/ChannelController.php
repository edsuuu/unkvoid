<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Enums\ChannelTypeEnum;
use App\Http\Requests\Api\Servers\StoreChannelRequest;
use App\Http\Requests\Api\Servers\UpdateChannelRequest;
use App\Http\Resources\Api\ChannelResource;
use App\Models\Channel;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

/**
 * Canais de texto e de voz de um servidor.
 */
final class ChannelController
{
    /**
     * @throws Throwable
     */
    public function store(StoreChannelRequest $request, Server $server, #[CurrentUser] User $user): ChannelResource
    {
        $topic = $request->string('topic')->toString();

        $channel = $server->createChannel(
            $user,
            $request->string('name')->toString(),
            ChannelTypeEnum::from($request->string('type')->toString()),
            $topic === '' ? null : $topic,
            $request->filled('user_limit') ? $request->integer('user_limit') : null,
        );

        return new ChannelResource($channel);
    }

    /**
     * @throws Throwable
     */
    public function update(UpdateChannelRequest $request, Channel $channel, #[CurrentUser] User $user): ChannelResource
    {
        /** @var array{name?: string, topic?: ?string, position?: int, user_limit?: ?int} $changes */
        $changes = $request->validated();

        $channel->change($user, $changes);

        return new ChannelResource($channel->refresh());
    }

    /**
     * @throws Throwable
     */
    public function destroy(Channel $channel, #[CurrentUser] User $user): Response
    {
        $channel->remove($user);

        return response()->noContent();
    }
}
