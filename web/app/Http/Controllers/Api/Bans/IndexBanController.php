<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Bans;

use App\Enums\PermissionEnum;
use App\Http\Resources\Api\BanResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Throwable;

final class IndexBanController
{
    /**
     * @throws Throwable
     */
    public function __invoke(Server $server, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        $server->memberOrFail($user)->authorize(PermissionEnum::BanMembers);

        return BanResource::collection($server->bans()->with('user')->orderByDesc('id')->get());
    }
}
