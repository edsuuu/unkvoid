<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Http\Requests\Api\Servers\StoreServerIconRequest;
use App\Http\Resources\Api\ServerResource;
use App\Models\Server;
use App\Models\User;
use App\Services\Storage\BucketService;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\UploadedFile;
use Throwable;

final class StoreServerIconController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreServerIconRequest $request, Server $server, #[CurrentUser] User $user, BucketService $bucket): ServerResource
    {
        /** @var UploadedFile $icon */
        $icon = $request->file('icon');

        $server->setIcon($user, $icon, $bucket);

        return new ServerResource($server);
    }
}
