<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Exceptions\ForbiddenException;
use App\Http\Requests\Api\Servers\StoreServerIconRequest;
use App\Http\Requests\Api\Servers\StoreServerRequest;
use App\Http\Requests\Api\Servers\UpdateServerRequest;
use App\Http\Resources\Api\AuditResource;
use App\Http\Resources\Api\InviteResource;
use App\Http\Resources\Api\ServerResource;
use App\Http\Resources\Api\ServerTreeResource;
use App\Models\Server;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use App\Services\Storage\BucketService;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Illuminate\Http\UploadedFile;
use Throwable;

/**
 * Servidor: a árvore, o convite, o ícone, a auditoria e a saída.
 */
final class ServerController
{
    public function index(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ServerResource::collection(Server::listFor($user));
    }

    /**
     * @throws Throwable
     */
    public function show(Server $server, #[CurrentUser] User $user, SfuClient $sfu): ServerTreeResource
    {
        return new ServerTreeResource($server, $server->memberOrFail($user), $sfu);
    }

    /**
     * @throws Throwable
     */
    public function store(StoreServerRequest $request, #[CurrentUser] User $user): ServerResource
    {
        return new ServerResource(Server::createFor($user, $request->string('name')->toString()));
    }

    /**
     * @throws Throwable
     */
    public function update(UpdateServerRequest $request, Server $server, #[CurrentUser] User $user): ServerResource
    {
        $server->rename($user, $request->string('name')->toString());

        return new ServerResource($server);
    }

    /**
     * @throws Throwable
     */
    public function destroy(Server $server, #[CurrentUser] User $user): Response
    {
        $server->destroyBy($user);

        return response()->noContent();
    }

    /**
     * @throws Throwable
     */
    public function leave(Server $server, #[CurrentUser] User $user, SfuClient $sfu): Response
    {
        $server->leave($user, $sfu);

        return response()->noContent();
    }

    /**
     * @throws Throwable
     */
    public function regenerateInvite(Server $server, #[CurrentUser] User $user): InviteResource
    {
        return new InviteResource($server->regenerateInvite($user));
    }

    /**
     * @throws Throwable
     */
    public function storeIcon(StoreServerIconRequest $request, Server $server, #[CurrentUser] User $user, BucketService $bucket): ServerResource
    {
        /** @var UploadedFile $icon */
        $icon = $request->file('icon');

        $server->setIcon($user, $icon, $bucket);

        return new ServerResource($server);
    }

    /**
     * @throws Throwable
     */
    public function destroyIcon(Server $server, #[CurrentUser] User $user): Response
    {
        $server->removeIcon($user);

        return response()->noContent();
    }

    /**
     * @throws ForbiddenException
     */
    public function audits(Server $server, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        return AuditResource::collection($server->history($user));
    }

    /**
     * @throws Throwable
     */
    public function join(string $code, #[CurrentUser] User $user): ServerResource
    {
        return new ServerResource(Server::joinByInvite($user, $code));
    }
}
