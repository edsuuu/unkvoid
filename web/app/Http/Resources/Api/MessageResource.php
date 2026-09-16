<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\Message;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Str;

/**
 * @mixin Message
 */
final class MessageResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'channel_id' => $this->channel_id,
            'user' => ['id' => $this->user->id, 'name' => $this->user->name, 'avatar_url' => $this->user->avatar_url],
            'type' => $this->type->value,
            'body' => $this->body,
            'reply_to' => is_null($this->replyTo) ? null : [
                'id' => $this->replyTo->id,
                'name' => $this->replyTo->user->name,
                'body' => Str::limit($this->replyTo->body, 120),
            ],
            'edited_at' => $this->edited_at?->toIso8601String(),
            'created_at' => $this->created_at->toIso8601String(),
        ];
    }
}
