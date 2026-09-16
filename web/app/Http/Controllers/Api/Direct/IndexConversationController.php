<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Direct;

use App\Http\Resources\Api\ConversationResource;
use App\Models\DirectMessage;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;

final class IndexConversationController
{
    public function __invoke(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ConversationResource::collection(DirectMessage::conversationsFor($user));
    }
}
