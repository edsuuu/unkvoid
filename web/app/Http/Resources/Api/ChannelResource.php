<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\Channel;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin Channel
 */
final class ChannelResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'name' => $this->name,
            'type' => $this->type->value,
            'topic' => $this->topic,
            'position' => $this->position,
            'user_limit' => $this->user_limit,
        ];
    }
}
