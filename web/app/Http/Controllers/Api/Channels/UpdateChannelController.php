<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Channels;

use App\Http\Requests\Api\Servers\UpdateChannelRequest;
use App\Http\Resources\Api\ChannelResource;
use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class UpdateChannelController
{
    /**
     * @throws Throwable
     */
    public function __invoke(UpdateChannelRequest $request, Channel $channel, #[CurrentUser] User $user): ChannelResource
    {
        /** @var array{name?: string, topic?: ?string, position?: int, user_limit?: ?int} $changes */
        $changes = $request->validated();

        $channel->change($user, $changes);

        return new ChannelResource($channel->refresh());
    }
}
