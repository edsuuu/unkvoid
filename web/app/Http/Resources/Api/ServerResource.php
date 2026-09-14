<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\Server;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin Server
 */
final class ServerResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'name' => $this->name,
            'owner_id' => $this->owner_id,
            'last_accessed_at' => $this->last_accessed_at?->toIso8601String(),
        ];
    }
}
