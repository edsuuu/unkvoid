<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\DirectMessage;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin DirectMessage
 */
final class DirectMessageResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'body' => $this->body,
            'created_at' => $this->created_at->toIso8601String(),
            'edited_at' => $this->edited_at?->toIso8601String(),
            'mine' => $this->sender_id === $request->user()?->id,
            'sender' => ['id' => $this->sender->id, 'name' => $this->sender->name, 'avatar_url' => $this->sender->avatar_url],
        ];
    }
}
