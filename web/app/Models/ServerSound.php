<?php

declare(strict_types=1);

namespace App\Models;

use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Support\Facades\Storage;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;

/**
 * @property int $id
 * @property int $server_id
 * @property ?int $user_id
 * @property string $name
 * @property ?string $emoji
 * @property string $path
 * @property int $volume
 * @property CarbonImmutable $created_at
 * @property-read Server $server
 */
#[Fillable(['server_id', 'user_id', 'name', 'emoji', 'path', 'volume'])]
final class ServerSound extends Model implements Auditable
{
    use AuditableTrait;

    private const int LINK_HOURS = 2;

    /**
     * @return BelongsTo<Server, $this>
     */
    public function server(): BelongsTo
    {
        return $this->belongsTo(Server::class);
    }

    /**
     * O bucket é privado: a URL sai assinada e vence, como a do ícone.
     */
    public function url(): string
    {
        return Storage::disk('s3')->temporaryUrl($this->path, now()->addHours(self::LINK_HOURS));
    }
}
