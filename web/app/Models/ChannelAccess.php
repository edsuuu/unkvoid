<?php

declare(strict_types=1);

namespace App\Models;

use App\Models\Concerns\LogsFailedWrites;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Override;
use Throwable;

/**
 * @property int $id
 * @property string $channel_id
 * @property int $user_id
 * @property string $ip
 * @property ?string $sfu_ip
 * @property ?string $user_agent
 * @property CarbonImmutable $joined_at
 * @property ?CarbonImmutable $left_at
 * @property-read Channel $channel
 * @property-read User $user
 */
#[Fillable(['channel_id', 'user_id', 'ip', 'sfu_ip', 'user_agent', 'joined_at', 'left_at'])]
final class ChannelAccess extends Model
{
    use LogsFailedWrites;

    /**
     * Uma sessão sem `left` depois disso é um webhook perdido, não alguém em chamada.
     */
    private const int STALE_HOURS = 24;

    /**
     * Fecha o que estava aberto para a mesma pessoa no mesmo canal e abre um novo. Quando
     * o SFU avisa que entrou, o IP e o navegador vêm do pedido de token que o precedeu.
     * A limpeza dos acessos velhos anda junto com o que chega, e não num agendador: a VPS
     * não roda `schedule:run`.
     *
     * @throws Throwable
     */
    public static function open(Channel $channel, User $user, ?string $ip, ?string $userAgent, ?string $sfuIp, CarbonImmutable $at): self
    {
        return self::write('falha ao registrar a entrada na voz', function () use ($channel, $user, $ip, $userAgent, $sfuIp, $at): self {
            $previous = self::openFor($channel, $user)->first();
            $previous?->update(['left_at' => $at]);

            self::query()->whereNull('left_at')->where('joined_at', '<', $at->subHours(self::STALE_HOURS))->update(['left_at' => $at]);

            return self::query()->create([
                'channel_id' => $channel->id,
                'user_id' => $user->id,
                'ip' => $ip ?? $previous->ip ?? '',
                'sfu_ip' => $sfuIp,
                'user_agent' => $userAgent ?? $previous->user_agent ?? null,
                'joined_at' => $at,
            ]);
        }, ['channel_id' => $channel->id, 'user_id' => $user->id]);
    }

    /**
     * Um `left` que chega atrasado só fecha o que abriu antes dele, nunca o acesso mais novo.
     *
     * @throws Throwable
     */
    public static function close(Channel $channel, User $user, CarbonImmutable $at): void
    {
        self::write('falha ao registrar a saída da voz', fn () => self::openFor($channel, $user)->where('joined_at', '<', $at)->update(['left_at' => $at]), ['channel_id' => $channel->id, 'user_id' => $user->id]);
    }

    /**
     * @return BelongsTo<Channel, $this>
     */
    public function channel(): BelongsTo
    {
        return $this->belongsTo(Channel::class);
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'joined_at' => 'datetime',
            'left_at' => 'datetime',
        ];
    }

    /**
     * @return Builder<self>
     */
    private static function openFor(Channel $channel, User $user): Builder
    {
        return self::query()->where('channel_id', $channel->id)->where('user_id', $user->id)->whereNull('left_at')->orderByDesc('id');
    }
}
