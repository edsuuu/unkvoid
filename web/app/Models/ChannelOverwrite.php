<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\OverwriteTargetEnum;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;

/**
 * @property int $id
 * @property string $channel_id
 * @property OverwriteTargetEnum $target_type
 * @property int $target_id
 * @property int $allow
 * @property int $deny
 * @property-read Channel $channel
 */
#[Fillable(['channel_id', 'target_type', 'target_id', 'allow', 'deny'])]
final class ChannelOverwrite extends Model implements Auditable
{
    use AuditableTrait;

    /**
     * @return BelongsTo<Channel, $this>
     */
    public function channel(): BelongsTo
    {
        return $this->belongsTo(Channel::class);
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'target_type' => OverwriteTargetEnum::class,
            'target_id' => 'integer',
            'allow' => 'integer',
            'deny' => 'integer',
        ];
    }
}
