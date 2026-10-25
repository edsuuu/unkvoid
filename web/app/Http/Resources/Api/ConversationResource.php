<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Enums\FriendshipStatusEnum;
use App\Models\DirectMessage;
use App\Models\Friendship;
use App\Models\User;
use App\Services\Presence\ActivityPresence;
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
        $viewer = $request->user();
        abort_unless($viewer instanceof User, 401, 'É necessário entrar na conta.');

        $mine = $this->sender_id === $viewer->id;
        $other = $mine ? $this->recipient : $this->sender;
        $friendship = Friendship::between($viewer, $other);
        $isFriend = $friendship?->status === FriendshipStatusEnum::Accepted;

        return [
            'user' => ['id' => $other->id, 'name' => $other->name, 'avatar_url' => $other->avatar_url, 'status' => $other->publicStatus(), 'activity' => $isFriend ? resolve(ActivityPresence::class)->get($other) : null, 'created_at' => $other->created_at?->toIso8601String(), 'banner_color' => $other->banner_color],
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
