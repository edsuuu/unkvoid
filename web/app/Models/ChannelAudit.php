<?php

declare(strict_types=1);

namespace App\Models;

use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Request;
use Throwable;

/**
 * O histórico de um canal: criado, renomeado, apagado, com quem fez e de onde.
 *
 * @property int $id
 * @property string $channel_id
 * @property int $server_id
 * @property ?int $user_id
 * @property string $event
 * @property ?array<string, mixed> $old_values
 * @property ?array<string, mixed> $new_values
 * @property ?string $ip_address
 * @property ?string $user_agent
 * @property CarbonImmutable $created_at
 * @property-read ?User $user
 */
#[Fillable(['channel_id', 'server_id', 'user_id', 'event', 'old_values', 'new_values', 'ip_address', 'user_agent', 'created_at'])]
final class ChannelAudit extends Model
{
    /**
     * Linha de histórico não se altera, então só existe `created_at`, gravado à mão.
     */
    public $timestamps = false;

    /**
     * Registrar nunca pode derrubar a ação registrada.
     *
     * Foi exatamente isso que quebrou antes: a auditoria estourava no meio da transação
     * e levava a criação do canal com ela. Aqui a falha vira linha de log e a ação segue.
     *
     * @param  ?array<string, mixed>  $old
     * @param  ?array<string, mixed>  $new
     */
    public static function record(Channel $channel, string $event, ?array $old, ?array $new): void
    {
        try {
            self::query()->create([
                'channel_id' => $channel->id,
                'server_id' => $channel->server_id,
                'user_id' => Auth::id(),
                'event' => $event,
                'old_values' => $old,
                'new_values' => $new,
                'ip_address' => Request::ip(),
                'user_agent' => mb_substr((string) Request::userAgent(), 0, 1023) ?: null,
                'created_at' => now(),
            ]);
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] não deu para registrar o histórico do canal', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'channel_id' => $channel->id,
                'event' => $event,
            ]);
        }
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
    protected function casts(): array
    {
        return [
            'old_values' => 'array',
            'new_values' => 'array',
            'created_at' => 'datetime',
        ];
    }
}
