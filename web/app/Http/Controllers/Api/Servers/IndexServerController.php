<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Servers;

use App\Http\Resources\Api\ServerResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;

final class IndexServerController
{
    public function __invoke(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ServerResource::collection(Server::listFor($user));
    }
}
