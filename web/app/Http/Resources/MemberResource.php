<?php

declare(strict_types=1);

namespace App\Http\Resources;

use App\Models\ServerMember;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Override;

/** @mixin ServerMember */
final class MemberResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    #[Override]
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'user_id' => $this->user_id,
            'name' => $this->nickname ?? $this->user->displayName(),
            'avatar_url' => $this->user->avatar_url,
            'role' => $this->role,
            'joined_at' => $this->joined_at?->toIso8601String(),
        ];
    }
}
