<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Direct;

use App\Http\Requests\Api\Servers\IndexMessageRequest;
use App\Http\Resources\Api\DirectMessageResource;
use App\Models\DirectMessage;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Throwable;

final class ShowConversationController
{
    /**
     * @throws Throwable
     */
    public function __invoke(IndexMessageRequest $request, User $user, #[CurrentUser] User $viewer): AnonymousResourceCollection
    {
        $before = $request->filled('before') ? $request->integer('before') : null;

        return DirectMessageResource::collection(DirectMessage::conversation($viewer, $user, $before));
    }
}
