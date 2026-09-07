<?php

declare(strict_types=1);

namespace App\Http\Resources;

use App\Models\Message;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Override;

/** @mixin Message */
final class MessageResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    #[Override]
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'channel_id' => $this->channel_id,
            'content' => $this->content,
            'author' => [
                'id' => $this->user->id,
                'name' => $this->user->displayName(),
                'avatar_url' => $this->user->avatar_url,
            ],
            'created_at' => $this->created_at?->toIso8601String(),
        ];
    }
}
