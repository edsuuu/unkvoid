<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\PermissionEnum;
use App\Events\MessageDeleted;
use App\Events\MessageUpdated;
use App\Exceptions\ForbiddenException;
use App\Models\Concerns\LogsFailedWrites;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;
use Throwable;

/**
 * @property int $id
 * @property string $channel_id
 * @property int $user_id
 * @property string $body
 * @property ?CarbonImmutable $edited_at
 * @property CarbonImmutable $created_at
 * @property-read Channel $channel
 * @property-read User $user
 */
#[Fillable(['channel_id', 'user_id', 'body', 'edited_at'])]
final class Message extends Model implements Auditable
{
    use AuditableTrait;
    use LogsFailedWrites;

    /**
     * @return BelongsTo<Channel, $this>
     */
    public function channel(): BelongsTo
    {
        return $this->belongsTo(Channel::class);
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    /**
     * @throws Throwable
     */
    public function edit(User $actor, string $body): void
    {
        $this->channel->memberOrFail($actor)->authorize(PermissionEnum::ViewChannel, $this->channel);

        throw_if($this->user_id !== $actor->id, ForbiddenException::class, 'Só quem escreveu edita a mensagem.');

        self::write('falha ao editar a mensagem', fn () => $this->update(['body' => $body, 'edited_at' => now()]), ['message_id' => $this->id]);

        self::broadcast(new MessageUpdated($this->load('user')));
    }

    /**
     * @throws Throwable
     */
    public function remove(User $actor): void
    {
        $this->channel->memberOrFail($actor)->authorize(PermissionEnum::ViewChannel, $this->channel);

        if ($this->user_id !== $actor->id) {
            $this->channel->memberOrFail($actor)->authorize(PermissionEnum::ManageMessages, $this->channel);
        }

        self::write('falha ao apagar a mensagem', fn () => $this->delete(), ['message_id' => $this->id]);

        self::broadcast(new MessageDeleted($this->id, $this->channel_id));
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'edited_at' => 'datetime',
        ];
    }
}
