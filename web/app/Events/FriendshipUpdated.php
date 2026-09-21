<?php

declare(strict_types=1);

namespace App\Events;

use App\Http\Resources\Api\FriendResource;
use App\Models\Friendship;
use Illuminate\Foundation\Events\Dispatchable;

/**
 * Vai para os dois lados: quem pediu precisa ver "aceito" na hora, e quem recebeu precisa
 * ver o pedido chegar sem recarregar.
 */
final readonly class FriendshipUpdated implements SfuEvent
{
    use Dispatchable;

    public function __construct(public Friendship $friendship, public bool $removed = false) {}

    /**
     * @return array<int, string>
     */
    public function channels(): array
    {
        return ['user.'.$this->friendship->requester_id, 'user.'.$this->friendship->addressee_id];
    }

    public function eventName(): string
    {
        return 'FriendshipUpdated';
    }

    /**
     * @return array<string, mixed>
     */
    public function payload(): array
    {
        return [
            'friendship' => new FriendResource($this->friendship)->resolve(),
            'removed' => $this->removed,
        ];
    }
}
