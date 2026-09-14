<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Clips;

use App\Http\Requests\Api\Servers\StoreClipRequest;
use App\Http\Resources\Api\ClipResource;
use App\Models\Channel;
use App\Models\Clip;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\JsonResponse;
use Throwable;

final class StoreClipController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreClipRequest $request, Channel $channel, #[CurrentUser] User $user, SfuClient $sfu): JsonResponse
    {
        $streamer = User::query()->findOrFail($request->integer('user_id'));

        return new ClipResource(Clip::start($user, $channel, $streamer, $sfu))->response()->setStatusCode(202);
    }
}
