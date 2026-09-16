<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Audits;

use App\Exceptions\ForbiddenException;
use App\Http\Resources\Api\AuditResource;
use App\Models\Server;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;

final class IndexAuditController
{
    /**
     * @throws ForbiddenException
     */
    public function __invoke(Server $server, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        return AuditResource::collection($server->history($user));
    }
}
