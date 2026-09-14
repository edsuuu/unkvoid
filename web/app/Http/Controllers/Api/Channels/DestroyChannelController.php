<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Channels;

use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyChannelController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Channel $channel, #[CurrentUser] User $user): Response
    {
        $channel->remove($user);

        return response()->noContent();
    }
}
