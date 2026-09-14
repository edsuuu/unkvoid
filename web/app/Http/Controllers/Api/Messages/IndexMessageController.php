<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Messages;

use App\Http\Requests\Api\Servers\IndexMessageRequest;
use App\Http\Resources\Api\MessageResource;
use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Throwable;

final class IndexMessageController
{
    /**
     * @throws Throwable
     */
    public function __invoke(IndexMessageRequest $request, Channel $channel, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        $before = $request->filled('before') ? $request->integer('before') : null;

        return MessageResource::collection($channel->messagesBefore($user, $before));
    }
}
