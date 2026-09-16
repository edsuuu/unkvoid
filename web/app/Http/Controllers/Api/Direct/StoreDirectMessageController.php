<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Direct;

use App\Http\Requests\Api\Servers\StoreMessageRequest;
use App\Http\Resources\Api\DirectMessageResource;
use App\Models\DirectMessage;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class StoreDirectMessageController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreMessageRequest $request, User $user, #[CurrentUser] User $sender): DirectMessageResource
    {
        return new DirectMessageResource(DirectMessage::send($sender, $user, $request->string('body')->toString()));
    }
}
