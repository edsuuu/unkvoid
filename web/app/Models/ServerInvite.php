<?php

declare(strict_types=1);

namespace App\Models;

use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;

/**
 * @property int $id
 * @property int $server_id
 * @property ?int $user_id
 * @property ?string $channel_id
 * @property string $code
 * @property int $uses
 * @property ?int $max_uses
 * @property ?CarbonImmutable $expires_at
 * @property CarbonImmutable $created_at
 * @property-read Server $server
 * @property-read ?User $user
 * @property-read ?Channel $channel
 */
#[Fillable(['server_id', 'user_id', 'channel_id', 'code', 'uses', 'max_uses', 'expires_at'])]
final class ServerInvite extends Model implements Auditable
{
    use AuditableTrait;

    /**
     * @return BelongsTo<Server, $this>
     */
    public function server(): BelongsTo
    {
        return $this->belongsTo(Server::class);
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    /**
     * @return BelongsTo<Channel, $this>
     */
    public function channel(): BelongsTo
    {
        return $this->belongsTo(Channel::class);
    }

    /**
     * Vencido ou já usado todas as vezes: o convite existe, mas não deixa entrar.
     */
    public function spent(): bool
    {
        if (! is_null($this->expires_at) && $this->expires_at->isPast()) {
            return true;
        }

        return ! is_null($this->max_uses) && $this->uses >= $this->max_uses;
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'uses' => 'integer',
            'max_uses' => 'integer',
            'expires_at' => 'datetime',
        ];
    }
}
