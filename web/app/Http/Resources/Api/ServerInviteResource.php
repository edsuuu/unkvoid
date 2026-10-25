<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\ServerInvite;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin ServerInvite
 */
final class ServerInviteResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'code' => $this->code,
            'url' => url('/convite/'.$this->code),
            'uses' => $this->uses,
            'max_uses' => $this->max_uses,
            'expires_at' => $this->expires_at?->toIso8601String(),
            'channel' => is_null($this->channel) ? null : ['id' => $this->channel->id, 'name' => $this->channel->name],
            'creator' => is_null($this->user) ? null : ['id' => $this->user->id, 'name' => $this->user->name, 'avatar_url' => $this->user->avatar_url],
            'created_at' => $this->created_at->toIso8601String(),
        ];
    }
}
