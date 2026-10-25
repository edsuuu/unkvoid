<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\User;
use App\Services\Presence\ActivityPresence;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Facades\Date;

/**
 * @mixin User
 */
final class UserResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        $statusUntil = $this->getRawOriginal('status_until');
        /** @var User $user */
        $user = $this->resource;

        return [
            'id' => $this->id,
            'name' => $this->name,
            'email' => $this->email,
            'avatar_url' => $this->avatar_url,
            'avatar_uploaded' => ! is_null($this->avatar_id),
            'admin' => $this->isAdmin(),
            'nickname_confirmed' => $this->hasConfirmedNickname(),
            // Entrou só com o nome e ainda não pôs e-mail e senha: o e-mail acima é de mentira.
            'name_only' => $this->hasPlaceholderEmail(),
            'bio' => $this->bio,
            'pronouns' => $this->pronouns,
            'banner_color' => $this->banner_color,
            'custom_status' => $this->custom_status,
            'status' => $this->effectiveStatus(),
            'status_until' => is_string($statusUntil) ? Date::parse($statusUntil)->toIso8601String() : null,
            'activity' => resolve(ActivityPresence::class)->get($user),
            'created_at' => $this->created_at?->toIso8601String(),
        ];
    }
}
