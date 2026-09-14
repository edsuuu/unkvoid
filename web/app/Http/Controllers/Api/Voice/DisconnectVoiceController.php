<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Voice;

use App\Models\Channel;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DisconnectVoiceController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Channel $channel, User $user, #[CurrentUser] User $actor, SfuClient $sfu): Response
    {
        $channel->disconnect($actor, $user, $sfu);

        return response()->noContent();
    }
}
