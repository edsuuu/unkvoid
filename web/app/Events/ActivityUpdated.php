<?php

declare(strict_types=1);

namespace App\Events;

use App\Enums\FriendshipStatusEnum;
use App\Models\Friendship;
use App\Models\ServerMember;
use App\Models\User;
use Illuminate\Foundation\Events\Dispatchable;

final readonly class ActivityUpdated implements SfuEvent
{
    use Dispatchable;

    /**
     * @param  array{name: string, started_at: string}|null  $activity
     */
    public function __construct(private User $user, private ?array $activity) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        $channels = Friendship::query()
            ->where('status', FriendshipStatusEnum::Accepted)
            ->where(fn ($query) => $query->where('requester_id', $this->user->id)->orWhere('addressee_id', $this->user->id))
            ->get(['requester_id', 'addressee_id'])
            ->map(fn (Friendship $friendship): string => 'friend.'.($friendship->requester_id === $this->user->id ? $friendship->addressee_id : $friendship->requester_id))
            ->all();

        foreach (ServerMember::query()->where('user_id', $this->user->id)->get(['server_id']) as $membership) {
            $channels[] = 'server.'.$membership->server_id;
        }

        return array_values(array_unique($channels));
    }

    public function eventName(): string
    {
        return 'ActivityUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return ['user_id' => $this->user->id, 'activity' => $this->activity];
    }
}
