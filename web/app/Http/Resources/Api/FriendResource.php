<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\Friendship;
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
            'requester' => ['id' => $this->requester->id, 'name' => $this->requester->name, 'avatar_url' => $this->requester->avatar_url],
            'addressee' => ['id' => $this->addressee->id, 'name' => $this->addressee->name, 'avatar_url' => $this->addressee->avatar_url],
            'responded_at' => $this->responded_at?->toIso8601String(),
            'created_at' => $this->created_at?->toIso8601String(),
        ];
    }
}
