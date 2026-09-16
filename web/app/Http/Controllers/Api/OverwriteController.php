<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Enums\OverwriteTargetEnum;
use App\Http\Requests\Api\Servers\PutOverwriteRequest;
use App\Http\Resources\Api\OverwriteResource;
use App\Models\Channel;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

/**
 * Sobrescrita de permissão por cargo ou por membro num canal — é assim que se oculta canal.
 */
final class OverwriteController
{
    /**
     * @throws Throwable
     */
    public function put(PutOverwriteRequest $request, Channel $channel, OverwriteTargetEnum $type, int $id, #[CurrentUser] User $user): OverwriteResource
    {
        return new OverwriteResource($channel->putOverwrite($user, $type, $id, $request->integer('allow'), $request->integer('deny')));
    }

    /**
     * @throws Throwable
     */
    public function destroy(Channel $channel, OverwriteTargetEnum $type, int $id, #[CurrentUser] User $user): Response
    {
        $channel->removeOverwrite($user, $type, $id);

        return response()->noContent();
    }
}
