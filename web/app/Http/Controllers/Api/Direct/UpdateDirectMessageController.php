<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Direct;

use App\Http\Requests\Api\Servers\UpdateMessageRequest;
use App\Http\Resources\Api\DirectMessageResource;
use App\Models\DirectMessage;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class UpdateDirectMessageController
{
    /**
     * @throws Throwable
     */
    public function __invoke(UpdateMessageRequest $request, DirectMessage $directMessage, #[CurrentUser] User $user): DirectMessageResource
    {
        $directMessage->edit($user, $request->string('body')->toString());

        return new DirectMessageResource($directMessage);
    }
}
