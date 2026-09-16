<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Enums\PermissionEnum;
use App\Http\Requests\Api\Servers\StoreBanRequest;
use App\Http\Resources\Api\BanResource;
use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Throwable;

/**
 * Banidos de um servidor: listar, banir e perdoar.
 */
final class BanController
{
    /**
     * @throws Throwable
     */
    public function index(Server $server, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        $server->memberOrFail($user)->authorize(PermissionEnum::BanMembers);

        return BanResource::collection($server->bans()->with('user')->orderByDesc('id')->get());
    }

    /**
     * @throws Throwable
     */
    public function store(StoreBanRequest $request, Server $server, User $user, #[CurrentUser] User $actor, SfuClient $sfu): BanResource
    {
        $reason = $request->string('reason')->toString();

        return new BanResource($server->ban($actor, $user, $reason === '' ? null : $reason, $sfu));
    }

    /**
     * @throws Throwable
     */
    public function destroy(Server $server, User $user, #[CurrentUser] User $actor): Response
    {
        $server->unban($actor, $user);

        return response()->noContent();
    }
}
