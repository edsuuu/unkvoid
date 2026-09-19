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
use Illuminate\Database\Eloquent\Collection;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use Illuminate\Database\Eloquent\SoftDeletes;
use Illuminate\Validation\ValidationException;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;
use Throwable;

/**
 * @property int $id
 * @property string $channel_id
 * @property int $user_id
 * @property MessageTypeEnum $type
 * @property ?int $reply_to_id
 * @property string $body
 * @property ?CarbonImmutable $edited_at
 * @property CarbonImmutable $created_at
 * @property-read Channel $channel
 * @property-read User $user
 * @property-read ?Message $replyTo
 * @property-read Collection<int, File> $files
 */
#[Fillable(['channel_id', 'user_id', 'type', 'reply_to_id', 'body', 'edited_at'])]
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
     * @return BelongsTo<self, $this>
     */
    public function replyTo(): BelongsTo
    {
        return $this->belongsTo(self::class, 'reply_to_id');
    }

    /**
     * As imagens da mensagem, na ordem em que subiram.
     *
     * @return BelongsToMany<File, $this>
     */
    public function files(): BelongsToMany
    {
        return $this->belongsToMany(File::class, 'message_files')->orderBy('files.id');
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

        // Depois da permissão, e não no FormRequest: recusar antes diria a quem não enxerga
        // o canal se a mensagem tem imagem.
        throw_if($body === '' && $this->files->isEmpty(), ValidationException::withMessages(['body' => 'A mensagem precisa de texto ou de imagem.']));

        self::write('falha ao editar a mensagem', fn () => $this->update(['body' => $body, 'edited_at' => now()]), ['message_id' => $this->id]);

        self::broadcast(new MessageUpdated($this->load(['user', 'files'])));
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

        // Apagar é soft delete, então a pivô não cai sozinha: é a linha de `files` que leva
        // a pivô junto. Falha no bucket fica no log (`File::forget`) e não desfaz a exclusão.
        foreach ($this->files as $file) {
            $file->forget();
        }
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
