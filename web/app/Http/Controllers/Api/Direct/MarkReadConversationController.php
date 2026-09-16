<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Direct;

use App\Models\DirectMessage;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class MarkReadConversationController
{
    /**
     * @throws Throwable
     */
    public function __invoke(User $user, #[CurrentUser] User $viewer): Response
    {
        DirectMessage::markRead($viewer, $user);

        return response()->noContent();
    }
}
