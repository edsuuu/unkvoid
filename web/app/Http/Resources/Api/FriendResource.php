<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\Friendship;
use App\Models\User;
use App\Services\Presence\ActivityPresence;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * Os dois lados vão na resposta, e não "a outra pessoa": quem pediu importa para a
 * interface saber se mostra "aceitar" ou "aguardando".
 *
 * @mixin Friendship
 */
final class FriendResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'status' => $this->status->value,
            'requester' => $this->person($this->requester, $this->status->value === 'accepted'),
            'addressee' => $this->person($this->addressee, $this->status->value === 'accepted'),
            'responded_at' => $this->responded_at?->toIso8601String(),
            'created_at' => $this->created_at?->toIso8601String(),
        ];
    }

    /**
     * @return array<string, mixed>
     */
    private function person(User $user, bool $isFriend): array
    {
        return [
            'id' => $user->id,
            'name' => $user->name,
            'avatar_url' => $user->avatar_url,
            'status' => $user->publicStatus(),
            'activity' => $isFriend ? resolve(ActivityPresence::class)->get($user) : null,
            'created_at' => $user->created_at?->toIso8601String(),
            'banner_color' => $user->banner_color,
        ];
    }
}
