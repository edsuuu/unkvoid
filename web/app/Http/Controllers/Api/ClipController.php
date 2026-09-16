<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Servers\StoreClipRequest;
use App\Http\Resources\Api\ClipResource;
use App\Models\Channel;
use App\Models\Clip;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use App\Services\Storage\BucketService;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Throwable;

/**
 * Clipes: o que foi gravado da tela de alguém, e a playlist assinada que o toca.
 */
final class ClipController
{
    public function index(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ClipResource::collection(Clip::listFor($user));
    }

    public function show(string $clip, #[CurrentUser] User $user): ClipResource
    {
        return new ClipResource(Clip::findFor($user, $clip));
    }

    /**
     * @throws Throwable
     */
    public function store(StoreClipRequest $request, Channel $channel, #[CurrentUser] User $user, SfuClient $sfu, BucketService $bucket): JsonResponse
    {
        $streamer = User::query()->findOrFail($request->integer('user_id'));

        return new ClipResource(Clip::start($user, $channel, $streamer, $sfu, $bucket))->response()->setStatusCode(202);
    }

    /**
     * @throws Throwable
     */
    public function destroy(string $clip, #[CurrentUser] User $user): Response
    {
        Clip::findFor($user, $clip)->remove();

        return response()->noContent();
    }

    public function playlist(string $clip): Response
    {
        $playlist = Clip::findReady($clip)->playlist();

        abort_if(is_null($playlist), 404);

        return response($playlist, 200, ['Content-Type' => 'application/vnd.apple.mpegurl']);
    }
}
