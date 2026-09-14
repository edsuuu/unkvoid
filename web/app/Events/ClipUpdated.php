<?php

declare(strict_types=1);

namespace App\Events;

use App\Http\Resources\Api\ClipResource;
use App\Models\Clip;
use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Foundation\Events\Dispatchable;

final class ClipUpdated implements ShouldBroadcastNow
{
    use Dispatchable;
    use InteractsWithSockets;

    public function __construct(public readonly Clip $clip) {}

    public function broadcastOn(): PrivateChannel
    {
        return new PrivateChannel('user.'.$this->clip->user_id);
    }

    public function broadcastAs(): string
    {
        return 'ClipUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function broadcastWith(): array
    {
        return ['clip' => new ClipResource($this->clip)->resolve()];
    }
}
