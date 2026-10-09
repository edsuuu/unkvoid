<?php

declare(strict_types=1);

namespace App\Models;

use App\Models\Concerns\LogsFailedWrites;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Query\JoinClause;
use Throwable;

/**
 * Até onde a pessoa leu o canal. A marca só anda para a frente.
 *
 * @property int $id
 * @property int $user_id
 * @property string $channel_id
 * @property int $last_read_id
 */
#[Fillable(['user_id', 'channel_id', 'last_read_id'])]
final class ChannelRead extends Model
{
    use LogsFailedWrites;

    /**
     * Sem `$upTo`, lido até a mensagem mais recente do canal, apagada ou não: o id é só
     * uma marca na régua.
     *
     * @throws Throwable
     */
    public static function mark(Channel $channel, User $user, ?int $upTo): void
    {
        $latest = $channel->messages()->withTrashed()->max('id');
        $upTo ??= is_numeric($latest) ? (int) $latest : 0;

        self::write('falha ao marcar o canal como lido', function () use ($channel, $user, $upTo): void {
            $read = self::query()->firstOrNew(['user_id' => $user->id, 'channel_id' => $channel->id]);

            if ($upTo > $read->last_read_id) {
                $read->fill(['last_read_id' => $upTo])->save();
            }
        }, ['channel_id' => $channel->id, 'user_id' => $user->id]);
    }

    /**
     * Quantas mensagens de outras pessoas chegaram depois da marca, por canal. Canal sem
     * nada novo não aparece.
     *
     * ponytail: é um `count` sobre as mensagens de cada canal a cada árvore; num servidor
     * com anos de histórico e uma pessoa que nunca leu, a conta varre tudo. Se pesar, um
     * teto (`min(total, 99)`) ou um contador por canal gravado no `MessageSent`.
     *
     * @param  array<int, string>  $channelIds
     * @return array<string, int>
     */
    public static function unreadFor(int $userId, array $channelIds): array
    {
        if ($channelIds === []) {
            return [];
        }

        /** @var array<string, int> $unread */
        $unread = Message::query()
            ->selectRaw('messages.channel_id, count(*) as total')
            ->leftJoin('channel_reads', fn (JoinClause $join) => $join->on('channel_reads.channel_id', '=', 'messages.channel_id')->where('channel_reads.user_id', $userId))
            ->whereIn('messages.channel_id', $channelIds)
            ->where('messages.user_id', '!=', $userId)
            ->whereRaw('messages.id > coalesce(channel_reads.last_read_id, 0)')
            ->groupBy('messages.channel_id')
            ->pluck('total', 'messages.channel_id')
            ->map(fn (mixed $total): int => is_numeric($total) ? (int) $total : 0)
            ->all();

        return $unread;
    }
}
