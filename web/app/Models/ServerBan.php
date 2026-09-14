<?php

declare(strict_types=1);

namespace App\Models;

use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;

/**
 * @property int $id
 * @property int $server_id
 * @property int $user_id
 * @property ?int $banned_by
 * @property ?string $reason
 * @property CarbonImmutable $created_at
 * @property-read Server $server
 * @property-read User $user
 */
#[Fillable(['server_id', 'user_id', 'banned_by', 'reason'])]
final class ServerBan extends Model implements Auditable
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
}
