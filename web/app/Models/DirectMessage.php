<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\FriendshipStatusEnum;
use App\Events\DirectMessageCreated;
use App\Events\DirectMessageDeleted;
use App\Events\DirectMessageUpdated;
use App\Exceptions\ForbiddenException;
use App\Models\Concerns\LogsFailedWrites;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Collection;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\SoftDeletes;
use Illuminate\Support\Collection as SupportCollection;
use Override;
use Throwable;

/**
 * @property int $id
 * @property int $sender_id
 * @property int $recipient_id
 * @property string $body
 * @property ?CarbonImmutable $edited_at
 * @property ?CarbonImmutable $read_at
 * @property CarbonImmutable $created_at
 * @property int $unread só em `conversationsFor`
 * @property-read User $sender
 * @property-read User $recipient
 */
#[Fillable(['sender_id', 'recipient_id', 'body', 'edited_at', 'read_at'])]
final class DirectMessage extends Model
{
    use LogsFailedWrites;
    use SoftDeletes;

    private const int PAGE = 50;

    /**
     * Uma linha por conversa: a última mensagem de cada par, com quantas dela ainda não
     * li. A lista não exige amizade — desfazer a amizade tira a conversa do ar, não o
     * que já foi dito.
     *
     * @return Collection<int, self>
     */
    public static function conversationsFor(User $user): Collection
    {
        $latest = self::query()
            ->selectRaw('max(id) as id')
            ->where(fn (Builder $query) => $query->where('sender_id', $user->id)->orWhere('recipient_id', $user->id))
            ->groupByRaw('case when sender_id = ? then recipient_id else sender_id end', [$user->id])
            ->get()
            ->pluck('id');

        /** @var SupportCollection<int, int> $unread */
        $unread = self::query()
            ->selectRaw('sender_id, count(*) as total')
            ->where('recipient_id', $user->id)
            ->whereNull('read_at')
            ->groupBy('sender_id')
            ->pluck('total', 'sender_id');

        $messages = self::query()->with(['sender', 'recipient'])->whereIn('id', $latest)->orderByDesc('id')->get();

        foreach ($messages as $message) {
            $message->setAttribute('unread', $unread[$message->other($user)->id] ?? 0);
        }

        return $messages;
    }

    /**
     * As 50 mais recentes antes de `$before`, em ordem crescente, e o que chegou para
     * mim fica lido.
     *
     * @return Collection<int, self>
     *
     * @throws Throwable
     */
    public static function conversation(User $viewer, User $other, ?int $before): Collection
    {
        self::friendsOrFail($viewer, $other);

        $query = self::between($viewer, $other)->with('sender')->orderByDesc('id')->limit(self::PAGE);

        if (! is_null($before)) {
            $query->where('id', '<', $before);
        }

        $messages = $query->get();

        self::markRead($viewer, $other);

        return $messages->reverse()->values();
    }

    /**
     * Marca como lidas as mensagens que a outra pessoa mandou.
     *
     * Existe fora do `conversation` porque quem está com a conversa aberta recebe por
     * tempo real, sem pedir a página de novo: sem isto o contador de não lidas voltava
     * do banco na próxima vez que o app abrisse.
     *
     * @throws Throwable
     */
    public static function markRead(User $viewer, User $other): void
    {
        self::friendsOrFail($viewer, $other);

        self::write('falha ao marcar a conversa como lida', fn () => self::query()
            ->where('sender_id', $other->id)
            ->where('recipient_id', $viewer->id)
            ->whereNull('read_at')
            ->update(['read_at' => now()]), ['user_id' => $viewer->id, 'other_id' => $other->id]);
    }

    /**
     * @throws Throwable
     */
    public static function send(User $sender, User $recipient, string $body): self
    {
        self::friendsOrFail($sender, $recipient);

        $message = self::write('falha ao mandar a mensagem direta', fn (): self => self::query()->create([
            'sender_id' => $sender->id,
            'recipient_id' => $recipient->id,
            'body' => $body,
        ]), ['sender_id' => $sender->id, 'recipient_id' => $recipient->id]);

        $message->setRelation('sender', $sender);
        $message->setRelation('recipient', $recipient);

        self::broadcast(new DirectMessageCreated($message));

        return $message;
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function sender(): BelongsTo
    {
        return $this->belongsTo(User::class, 'sender_id');
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function recipient(): BelongsTo
    {
        return $this->belongsTo(User::class, 'recipient_id');
    }

    public function other(User $user): User
    {
        return $this->sender_id === $user->id ? $this->recipient : $this->sender;
    }

    /**
     * @throws Throwable
     */
    public function edit(User $actor, string $body): void
    {
        throw_if($this->sender_id !== $actor->id, ForbiddenException::class, 'Só quem escreveu edita a mensagem.');

        self::write('falha ao editar a mensagem direta', fn () => $this->update(['body' => $body, 'edited_at' => now()]), ['direct_message_id' => $this->id]);

        self::broadcast(new DirectMessageUpdated($this->load(['sender', 'recipient'])));
    }

    /**
     * @throws Throwable
     */
    public function remove(User $actor): void
    {
        throw_if($this->sender_id !== $actor->id, ForbiddenException::class, 'Só quem escreveu apaga a mensagem.');

        self::write('falha ao apagar a mensagem direta', fn () => $this->delete(), ['direct_message_id' => $this->id]);

        self::broadcast(new DirectMessageDeleted($this->id, $this->sender_id, $this->recipient_id));
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'edited_at' => 'datetime',
            'read_at' => 'datetime',
        ];
    }

    /**
     * Os dois sentidos do par num parêntese só: sem ele, o `before` da paginação grudaria
     * em metade do `or` e traria a conversa inteira.
     *
     * @return Builder<self>
     */
    private static function between(User $one, User $other): Builder
    {
        return self::query()->where(function (Builder $query) use ($one, $other): void {
            $query->where(fn (Builder $pair) => $pair->where('sender_id', $one->id)->where('recipient_id', $other->id))
                ->orWhere(fn (Builder $pair) => $pair->where('sender_id', $other->id)->where('recipient_id', $one->id));
        });
    }

    /**
     * Quem pode conversar: amigo aceito, ou gente do mesmo servidor.
     *
     * O servidor em comum existe porque a ficha de perfil de um membro tem campo de
     * mensagem: exigir amizade ali daria 403 em todo mundo que ainda não é amigo, que é
     * justamente quem a pessoa quer chamar. Bloqueio vence os dois.
     *
     * @throws ForbiddenException
     */
    private static function friendsOrFail(User $one, User $other): void
    {
        $friendship = Friendship::between($one, $other);

        throw_if(
            $friendship?->status === FriendshipStatusEnum::Blocked,
            ForbiddenException::class,
            'Esta conversa está bloqueada.'
        );

        if ($friendship?->status === FriendshipStatusEnum::Accepted) {
            return;
        }

        throw_if(
            ! self::shareAServer($one, $other),
            ForbiddenException::class,
            'Vocês não são amigos nem estão no mesmo servidor.'
        );
    }

    private static function shareAServer(User $one, User $other): bool
    {
        return ServerMember::query()
            ->where('user_id', $one->id)
            ->whereIn('server_id', ServerMember::query()->where('user_id', $other->id)->select('server_id'))
            ->exists();
    }
}
