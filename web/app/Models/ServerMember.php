<?php

declare(strict_types=1);

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Concerns\HasUuids;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Override;

#[Fillable(['server_id', 'user_id', 'nickname', 'role', 'joined_at'])]
final class ServerMember extends Model
{
    use HasUuids;

    public $timestamps = false;

    public function server(): BelongsTo
    {
        return $this->belongsTo(Server::class);
    }

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    public function canModerate(): bool
    {
        return in_array($this->role, ['owner', 'admin'], true);
    }

    #[Override]
    protected function casts(): array
    {
        return ['joined_at' => 'datetime'];
    }
}
