<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Messages;

use App\Models\Message;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyMessageController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Message $message, #[CurrentUser] User $user): Response
    {
        $message->remove($user);

        return response()->noContent();
    }
}
