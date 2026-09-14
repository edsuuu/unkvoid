<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Overwrites;

use App\Enums\OverwriteTargetEnum;
use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyOverwriteController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Channel $channel, OverwriteTargetEnum $type, int $id, #[CurrentUser] User $user): Response
    {
        $channel->removeOverwrite($user, $type, $id);

        return response()->noContent();
    }
}
