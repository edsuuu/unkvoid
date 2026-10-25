<?php

declare(strict_types=1);

namespace App\Events;

use App\Enums\FriendshipStatusEnum;
use App\Models\Friendship;
use App\Models\ServerMember;
use App\Models\User;
use Illuminate\Foundation\Events\Dispatchable;

final readonly class UserStatusUpdated implements SfuEvent
{
    use Dispatchable;

    public function __construct(private User $user) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        $channels = ['user.'.$this->user->id];
        $friendIds = Friendship::query()
            ->where('status', FriendshipStatusEnum::Accepted)
            ->where(fn ($query) => $query->where('requester_id', $this->user->id)->orWhere('addressee_id', $this->user->id))
            ->get(['requester_id', 'addressee_id'])
            ->map(fn (Friendship $friendship): int => $friendship->requester_id === $this->user->id ? $friendship->addressee_id : $friendship->requester_id);

        foreach ($friendIds as $friendId) {
            $channels[] = 'friend.'.$friendId;
        }

        $serverIds = ServerMember::query()->where('user_id', $this->user->id)->get(['server_id'])->map(
            fn (ServerMember $member): int => $member->server_id,
        );

        foreach ($serverIds as $serverId) {
            $channels[] = 'server.'.$serverId;
        }

        return array_values(array_unique($channels));
    }

    public function eventName(): string
    {
        return 'UserStatusUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['user_id' => $this->user->id, 'status' => $this->user->publicStatus()];
    }
}
