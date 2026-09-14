<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\ServerBan;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin ServerBan
 */
final class BanResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'user_id' => $this->user_id,
            'name' => $this->user->name,
            'reason' => $this->reason,
            'banned_by' => $this->banned_by,
            'created_at' => $this->created_at->toIso8601String(),
        ];
    }
}
