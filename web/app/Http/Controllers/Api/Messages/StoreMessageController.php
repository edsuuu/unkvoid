<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Messages;

use App\Http\Requests\Api\Servers\StoreMessageRequest;
use App\Http\Resources\Api\MessageResource;
use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class StoreMessageController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreMessageRequest $request, Channel $channel, #[CurrentUser] User $user): MessageResource
    {
        $replyToId = $request->filled('reply_to_id') ? $request->integer('reply_to_id') : null;

        return new MessageResource($channel->post($user, $request->string('body')->toString(), $replyToId));
    }
}
