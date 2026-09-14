<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Overwrites;

use App\Enums\OverwriteTargetEnum;
use App\Http\Requests\Api\Servers\PutOverwriteRequest;
use App\Http\Resources\Api\OverwriteResource;
use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Throwable;

final class PutOverwriteController
{
    /**
     * @throws Throwable
     */
    public function __invoke(PutOverwriteRequest $request, Channel $channel, OverwriteTargetEnum $type, int $id, #[CurrentUser] User $user): OverwriteResource
    {
        return new OverwriteResource($channel->putOverwrite($user, $type, $id, $request->integer('allow'), $request->integer('deny')));
    }
}
