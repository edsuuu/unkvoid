<?php

declare(strict_types=1);

namespace App\Http\Resources;

use App\Models\Server;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Override;

/** @mixin Server */
final class ServerResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    #[Override]
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'name' => $this->name,
            'initials' => $this->initials(),
            'icon_url' => $this->icon_path,
            'invite_url' => route('invite', ['code' => $this->invite_code]),
            'role' => $this->whenPivotLoaded('server_members', fn (): string => $this->pivot->role),
            'channels' => ChannelResource::collection($this->whenLoaded('channels')),
            'members' => MemberResource::collection($this->whenLoaded('members')),
        ];
    }
}
