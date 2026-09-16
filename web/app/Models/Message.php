<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\MessageTypeEnum;
use App\Enums\PermissionEnum;
use App\Events\MessageDeleted;
use App\Events\MessageUpdated;
use App\Exceptions\ForbiddenException;
use App\Models\Concerns\LogsFailedWrites;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\SoftDeletes;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;
use Throwable;

/**
 * @property int $id
 * @property string $channel_id
 * @property int $user_id
 * @property MessageTypeEnum $type
 * @property string $body
 * @property ?CarbonImmutable $edited_at
 * @property CarbonImmutable $created_at
 * @property-read Channel $channel
 * @property-read User $user
 */
#[Fillable(['channel_id', 'user_id', 'type', 'body', 'edited_at'])]
final class Message extends Model implements Auditable
{
    use AuditableTrait;
    use LogsFailedWrites;
    use SoftDeletes;

    /**
     * Mensagem de gente é o normal: quem não diz o tipo, diz `user`.
     *
     * @var array<string, string>
     */
    protected $attributes = ['type' => MessageTypeEnum::User->value];

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

        // Aviso de chegada leva o id de quem entrou, mas não é dele: é do servidor.
        throw_if($this->type !== MessageTypeEnum::User, ForbiddenException::class, 'Esta mensagem é do servidor.');
        throw_if($this->user_id !== $actor->id, ForbiddenException::class, 'Só quem escreveu edita a mensagem.');

        self::write('falha ao editar a mensagem', fn () => $this->update(['body' => $body, 'edited_at' => now()]), ['message_id' => $this->id]);

        self::broadcast(new MessageUpdated($this->load('user')));
    }

    /**
     * @throws Throwable
     */
    public function remove(User $actor): void
    {
        $member = $this->channel->memberOrFail($actor);

        $member->authorize(PermissionEnum::ViewChannel, $this->channel);

        // Aviso do servidor some pela moderação, não pela mão de quem chegou.
        if ($this->user_id !== $actor->id || $this->type !== MessageTypeEnum::User) {
            $member->authorize(PermissionEnum::ManageMessages, $this->channel);
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
            'type' => MessageTypeEnum::class,
            'edited_at' => 'datetime',
        ];
    }
}
