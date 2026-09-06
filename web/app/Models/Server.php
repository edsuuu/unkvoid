<?php

declare(strict_types=1);

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Concerns\HasUuids;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Support\Str;

#[Fillable(['owner_id', 'name', 'icon_path', 'invite_code'])]
final class Server extends Model
{
    use HasUuids;

    public static function generateInviteCode(): string
    {
        do {
            $code = Str::lower(Str::random(10));
        } while (self::where('invite_code', $code)->exists());

        return $code;
    }

    public function owner(): BelongsTo
    {
        return $this->belongsTo(User::class, 'owner_id');
    }

    public function members(): HasMany
    {
        return $this->hasMany(ServerMember::class);
    }

    public function channels(): HasMany
    {
        return $this->hasMany(Channel::class)->orderBy('position');
    }

    public function memberFor(User $user): ?ServerMember
    {
        return $this->members()->where('user_id', $user->id)->first();
    }

    public function initials(): string
    {
        return Str::of($this->name)
            ->explode(' ')
            ->take(2)
            ->map(fn (string $word): string => Str::upper(Str::substr($word, 0, 1)))
            ->implode('');
    }
}
