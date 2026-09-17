<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Sfu;

use App\Events\VoiceStateUpdated;
use App\Http\Requests\Api\Servers\SfuEventRequest;
use App\Models\Channel;
use App\Models\ChannelAccess;
use App\Models\Clip;
use App\Models\Concerns\LogsFailedWrites;
use App\Models\GuestAccess;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Carbon\CarbonImmutable;
use Illuminate\Http\Response;
use Throwable;

final class SfuEventController
{
    use LogsFailedWrites;

    /**
     * O SFU avisa quem entrou e saiu da voz, e como terminou um clipe. O visitante da sala
     * por código só vira linha de auditoria: não há conta nem canal para avisar.
     *
     * @throws Throwable
     */
    public function __invoke(SfuEventRequest $request, SfuClient $sfu): Response
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

        $at = CarbonImmutable::createFromTimestamp($request->integer('at'));
        $sub = $request->string('sub')->toString();

        $room = $request->string('room')->toString();

        // Sala por código: não há canal para avisar. O visitante vai pelo id da instalação e
        // quem entrou logado pela própria conta (`user:12`), na mesma tabela.
        if (mb_strlen($room) !== 26) {
            $installId = str_starts_with($sub, 'guest:') ? mb_substr($sub, mb_strlen('guest:')) : $sub;

            if ($event === 'joined') {
                GuestAccess::open($room, $installId, mb_substr($request->string('name')->toString(), 0, 40), $request->string('ip')->toString(), $at);
            } else {
                GuestAccess::close($room, $installId, $at);
            }

            return response()->noContent();
        }

        $channel = Channel::query()->findOrFail($room);
        $user = User::query()->findOrFail(User::fromSubject($sub));

        // Expulso ou banido depois de pedir o token (ele vale 60 s): o SFU deixou entrar, e a
        // voz derruba na hora, sem abrir acesso nem avisar o canal.
        if ($event === 'joined' && is_null($channel->server->memberOf($user))) {
            $sfu->kick($channel, $user->subject());

            return response()->noContent();
        }

        if ($event === 'joined') {
            ChannelAccess::open($channel, $user, null, null, $request->string('ip')->toString(), $at);
        } else {
            ChannelAccess::close($channel, $user, $at);
        }

        self::broadcast(new VoiceStateUpdated($channel->id, $user->id, $request->string('name')->toString(), $event));

        return response()->noContent();
    }
}
