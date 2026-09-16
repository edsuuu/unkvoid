<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Direct;

use App\Models\DirectMessage;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyDirectMessageController
{
    /**
     * @throws Throwable
     */
    public function __invoke(DirectMessage $directMessage, #[CurrentUser] User $user): Response
    {
        $directMessage->remove($user);

        return response()->noContent();
    }
}
