<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Sfu;

use App\Events\VoiceStateUpdated;
use App\Http\Requests\Api\Servers\SfuEventRequest;
use App\Models\Channel;
use App\Models\ChannelAccess;
use App\Models\Clip;
use App\Models\Concerns\LogsFailedWrites;
use App\Models\User;
use Carbon\CarbonImmutable;
use Illuminate\Http\Response;
use Throwable;

final class SfuEventController
{
    use LogsFailedWrites;

    /**
     * O SFU avisa quem entrou e saiu da voz, e como terminou um clipe. Sala anônima não
     * chega aqui.
     *
     * @throws Throwable
     */
    public function __invoke(SfuEventRequest $request): Response
    {
        $event = $request->string('event')->toString();

        if ($event === 'clip.ready') {
            Clip::markReady($request->string('clipId')->toString(), $request->integer('durationMs'), $request->integer('sizeBytes'));

            return response()->noContent();
        }

        if ($event === 'clip.failed') {
            Clip::markFailed($request->string('clipId')->toString(), $request->string('reason')->toString());

            return response()->noContent();
        }

        $channel = Channel::query()->findOrFail($request->string('room')->toString());
        $user = User::query()->findOrFail(User::fromSubject($request->string('sub')->toString()));
        $at = CarbonImmutable::createFromTimestamp($request->integer('at'));

        if ($event === 'joined') {
            ChannelAccess::open($channel, $user, null, null, $request->string('ip')->toString(), $at);
        } else {
            ChannelAccess::close($channel, $user, $at);
        }

        self::broadcast(new VoiceStateUpdated($channel->id, $user->id, $request->string('name')->toString(), $event));

        return response()->noContent();
    }
}
