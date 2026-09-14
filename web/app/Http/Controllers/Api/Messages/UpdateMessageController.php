<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Messages;

use App\Http\Requests\Api\Servers\UpdateMessageRequest;
use App\Http\Resources\Api\MessageResource;
use App\Models\Message;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class UpdateMessageController
{
    /**
     * @throws Throwable
     */
    public function __invoke(UpdateMessageRequest $request, Message $message, #[CurrentUser] User $user): MessageResource
    {
        $message->edit($user, $request->string('body')->toString());

        return new MessageResource($message);
    }
}
