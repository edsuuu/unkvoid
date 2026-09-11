<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\Release;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin Release
 */
final class ReleaseResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'version' => $this->version,
            'platform' => $this->platform->value,
            'file_name' => $this->file_name,
            'size' => $this->size,
            'published_at' => $this->published_at->toIso8601String(),
        ];
    }
}
