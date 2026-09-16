<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\DirectMessage;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * Uma conversa na lista: com quem, a última coisa dita e quantas eu ainda não li.
 *
 * @mixin DirectMessage
 */
final class ConversationResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        $mine = $this->sender_id === $request->user()?->id;
        $other = $mine ? $this->recipient : $this->sender;

        return [
            'user' => ['id' => $other->id, 'name' => $other->name, 'avatar_url' => $other->avatar_url],
            'last' => [
                'id' => $this->id,
                'body' => $this->body,
                'created_at' => $this->created_at->toIso8601String(),
                'mine' => $mine,
            ],
            'unread' => $this->unread,
        ];
    }
}
