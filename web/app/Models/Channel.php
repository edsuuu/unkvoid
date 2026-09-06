<?php

declare(strict_types=1);

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Concerns\HasUuids;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

#[Fillable(['server_id', 'name', 'type', 'position'])]
final class Channel extends Model
{
    use HasUuids;

    public function server(): BelongsTo
    {
        return $this->belongsTo(Server::class);
    }

    public function messages(): HasMany
    {
        return $this->hasMany(Message::class);
    }

    public function isVoice(): bool
    {
        return $this->type === 'voice';
    }
}
