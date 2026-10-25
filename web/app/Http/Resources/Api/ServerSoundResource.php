<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\ServerSound;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin ServerSound
 */
final class ServerSoundResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'name' => $this->name,
            'emoji' => $this->emoji,
            'volume' => $this->volume,
            'url' => $this->url(),
            'user_id' => $this->user_id,
            'created_at' => $this->created_at->toIso8601String(),
        ];
    }
}
