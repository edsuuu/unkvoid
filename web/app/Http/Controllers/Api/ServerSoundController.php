<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Servers\StoreServerSoundRequest;
use App\Http\Resources\Api\ServerSoundResource;
use App\Models\Server;
use App\Models\ServerSound;
use App\Models\User;
use App\Services\Storage\BucketService;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Illuminate\Http\UploadedFile;
use Throwable;

/**
 * Os sons do soundboard de um servidor: listar, enviar e apagar.
 */
final class ServerSoundController
{
    /**
     * @throws Throwable
     */
    public function index(Server $server, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ServerSoundResource::collection($server->soundsFor($user));
    }

    /**
     * @throws Throwable
     */
    public function store(StoreServerSoundRequest $request, Server $server, #[CurrentUser] User $user, BucketService $bucket): ServerSoundResource
    {
        /** @var UploadedFile $sound */
        $sound = $request->file('sound');
        $emoji = $request->string('emoji')->trim()->toString();

        return new ServerSoundResource($server->addSound(
            $user,
            $sound,
            $request->string('name')->trim()->toString(),
            $emoji === '' ? null : $emoji,
            $request->integer('volume', 100),
            $bucket,
        ));
    }

    /**
     * @throws Throwable
     */
    public function destroy(ServerSound $sound, #[CurrentUser] User $user): Response
    {
        $sound->server->removeSound($user, $sound);

        return response()->noContent();
    }
}
