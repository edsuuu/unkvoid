<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\ServerMember;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin ServerMember
 */
final class MemberResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'user_id' => $this->user_id,
            'name' => $this->user->name,
            'avatar_url' => $this->user->avatar_url,
            'nickname' => $this->nickname,
            'role_ids' => $this->roles->pluck('id')->all(),
            'server_mute' => $this->server_mute,
            'server_deaf' => $this->server_deaf,
            'is_owner' => $this->isOwner(),
        ];
    }
}
