<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Bans;

use App\Http\Requests\Api\Servers\StoreBanRequest;
use App\Http\Resources\Api\BanResource;
use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class StoreBanController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreBanRequest $request, Server $server, User $user, #[CurrentUser] User $actor, SfuClient $sfu): BanResource
    {
        $reason = $request->string('reason')->toString();

        return new BanResource($server->ban($actor, $user, $reason === '' ? null : $reason, $sfu));
    }
}
