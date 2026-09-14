<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Channels;

use App\Enums\ChannelTypeEnum;
use App\Http\Requests\Api\Servers\StoreChannelRequest;
use App\Http\Resources\Api\ChannelResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class StoreChannelController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreChannelRequest $request, Server $server, #[CurrentUser] User $user): ChannelResource
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
}
