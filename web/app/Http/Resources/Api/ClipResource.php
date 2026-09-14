<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\Clip;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin Clip
 */
final class ClipResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'status' => $this->status->value,
            'streamer' => ['id' => $this->streamer_user_id, 'name' => $this->streamer_name],
            'server_name' => $this->server_name,
            'channel_name' => $this->channel_name,
            'duration_ms' => $this->duration_ms,
            'size_bytes' => $this->size_bytes,
            'created_at' => $this->created_at->toIso8601String(),
            'expires_at' => $this->created_at->addDays(Clip::KEEP_DAYS)->toIso8601String(),
            'thumbnail_url' => $this->thumbnailUrl(),
            'playlist_url' => $this->playlistUrl(),
            'download_url' => $this->downloadUrl(),
        ];
    }
}
