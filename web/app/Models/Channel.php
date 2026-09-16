<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\ChannelTypeEnum;
use App\Enums\MessageTypeEnum;
use App\Enums\OverwriteTargetEnum;
use App\Enums\PermissionEnum;
use App\Events\MessageSent;
use App\Events\ServerUpdated;
use App\Exceptions\ForbiddenException;
use App\Models\Concerns\LogsFailedWrites;
use App\Services\Sfu\SfuClient;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Collection;
use Illuminate\Database\Eloquent\Concerns\HasUlids;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Support\Str;
use Illuminate\Validation\ValidationException;
use Override;
use Throwable;

/**
 * @property string $id
 * @property int $server_id
 * @property string $name
 * @property ChannelTypeEnum $type
 * @property ?string $topic
 * @property int $position
 * @property ?int $user_limit
 * @property-read Server $server
 * @property-read Collection<int, ChannelOverwrite> $overwrites
 * @property-read Collection<int, Message> $messages
 */
#[Fillable(['server_id', 'name', 'type', 'topic', 'position', 'user_limit'])]
final class Channel extends Model
{
    use HasUlids;
    use LogsFailedWrites;

    private const int PAGE = 50;

    /**
     * O id é a sala no SFU, e lá o código é `[a-z0-9]`: o ULID sai em minúsculas.
     */
    #[Override]
    public function newUniqueId(): string
    {
        return mb_strtolower((string) Str::ulid());
    }

    /**
     * @return BelongsTo<Server, $this>
     */
    public function server(): BelongsTo
    {
        return $this->belongsTo(Server::class);
    }

    /**
     * @return HasMany<ChannelOverwrite, $this>
     */
    public function overwrites(): HasMany
    {
        return $this->hasMany(ChannelOverwrite::class);
    }

    /**
     * @return HasMany<Message, $this>
     */
    public function messages(): HasMany
    {
        return $this->hasMany(Message::class);
    }

    /**
     * @throws ForbiddenException
     */
    public function memberOrFail(User $user): ServerMember
    {
        return $this->server->memberOrFail($user);
    }

    /**
     * @param  array{name?: string, topic?: ?string, position?: int, user_limit?: ?int}  $changes
     *
     * @throws Throwable
     */
    public function change(User $actor, array $changes): void
    {
        $this->memberOrFail($actor)->authorize(PermissionEnum::ManageChannels);

        if ($this->type === ChannelTypeEnum::Text && ! is_null($changes['user_limit'] ?? null)) {
            throw ValidationException::withMessages(['user_limit' => 'Só canal de voz tem limite de pessoas.']);
        }

        self::write('falha ao alterar o canal', fn () => $this->update($changes), ['channel_id' => $this->id]);

        self::broadcast(new ServerUpdated($this->server_id));
    }

    /**
     * @throws Throwable
     */
    public function remove(User $actor): void
    {
        $this->memberOrFail($actor)->authorize(PermissionEnum::ManageChannels);

        if ($this->type === ChannelTypeEnum::Text && $this->server->channels()->where('type', ChannelTypeEnum::Text)->count() === 1) {
            throw ValidationException::withMessages(['channel' => 'O servidor precisa de pelo menos um canal de texto.']);
        }

        self::write('falha ao apagar o canal', fn () => $this->delete(), ['channel_id' => $this->id]);

        self::broadcast(new ServerUpdated($this->server_id));
    }

    /**
     * Ninguém sobrescreve o que não tem neste canal: é o que impede alguém com MANAGE_ROLES
     * de se liberar um bit que o próprio cargo nega.
     *
     * @throws Throwable
     */
    public function putOverwrite(User $actor, OverwriteTargetEnum $type, int $targetId, int $allow, int $deny): ChannelOverwrite
    {
        $me = $this->memberOrFail($actor);
        $me->authorize(PermissionEnum::ManageRoles);

        // Os bits que a sobrescrita já tinha contam também: sem isto, zerar um `deny` que eu
        // não tenho (o VIEW_CHANNEL que esconde o canal) abria o canal para o servidor inteiro.
        $current = $this->overwrites()->where('target_type', $type)->where('target_id', $targetId)->first();

        throw_if((($allow | $deny | ($current->allow ?? 0) | ($current->deny ?? 0)) & ~$me->permissions($this)) !== 0, ForbiddenException::class, 'Você não pode sobrescrever permissões que não tem neste canal.');

        $this->authorizeOverwriteTarget($me, $type, $targetId);

        $overwrite = self::write('falha ao gravar a sobrescrita', fn (): ChannelOverwrite => $this->overwrites()->updateOrCreate(
            ['target_type' => $type, 'target_id' => $targetId],
            ['allow' => $allow, 'deny' => $deny],
        ), ['channel_id' => $this->id]);

        self::broadcast(new ServerUpdated($this->server_id));

        return $overwrite;
    }

    /**
     * @throws Throwable
     */
    public function removeOverwrite(User $actor, OverwriteTargetEnum $type, int $targetId): void
    {
        $me = $this->memberOrFail($actor);
        $me->authorize(PermissionEnum::ManageRoles);

        $this->authorizeOverwriteTarget($me, $type, $targetId);

        $current = $this->overwrites()->where('target_type', $type)->where('target_id', $targetId)->first();

        throw_if(((($current->allow ?? 0) | ($current->deny ?? 0)) & ~$me->permissions($this)) !== 0, ForbiddenException::class, 'Você não pode apagar permissões que não tem neste canal.');

        self::write('falha ao apagar a sobrescrita', fn () => $this->overwrites()->where('target_type', $type)->where('target_id', $targetId)->delete(), ['channel_id' => $this->id]);

        self::broadcast(new ServerUpdated($this->server_id));
    }

    /**
     * As 50 mais recentes antes de `$before`, em ordem crescente.
     *
     * @return Collection<int, Message>
     *
     * @throws ForbiddenException
     */
    public function messagesBefore(User $viewer, ?int $before): Collection
    {
        $this->memberOrFail($viewer)->authorize(PermissionEnum::ViewChannel, $this);

        $query = $this->messages()->with(['user', 'replyTo.user'])->orderByDesc('id')->limit(self::PAGE);

        if (! is_null($before)) {
            $query->where('id', '<', $before);
        }

        return $query->get()->reverse()->values();
    }

    /**
     * @throws Throwable
     */
    public function post(User $author, string $body, ?int $replyToId = null): Message
    {
        $member = $this->memberOrFail($author);
        $member->authorize(PermissionEnum::ViewChannel, $this);
        $member->authorize(PermissionEnum::SendMessages, $this);

        // Responder só vale dentro do mesmo canal: aceitar um id de fora vazaria o texto
        // de um canal que a pessoa talvez nem enxergue.
        $replyTo = is_null($replyToId) ? null : $this->messages()->with('user')->find($replyToId);

        throw_if(! is_null($replyToId) && is_null($replyTo), ValidationException::withMessages(['reply_to_id' => 'A mensagem respondida não é deste canal.']));

        $message = self::write('falha ao gravar a mensagem', fn (): Message => $this->messages()->create([
            'user_id' => $author->id,
            'reply_to_id' => $replyTo?->id,
            'body' => $body,
        ]), ['channel_id' => $this->id]);

        $message->setRelation('user', $author);
        $message->setRelation('replyTo', $replyTo);

        self::broadcast(new MessageSent($message));

        return $message;
    }

    /**
     * O aviso de chegada: sem corpo e sem permissão a conferir, porque quem escreve é o
     * servidor. A frase quem monta é o app, a partir de quem entrou.
     *
     * @throws Throwable
     */
    public function announceJoin(User $user): void
    {
        // O corpo existe para o app antigo, que não conhece `type` e mostraria um balão
        // vazio. O app novo lê o `type` e escreve a frase dele, ignorando isto.
        $message = self::write('falha ao avisar da chegada', fn (): Message => $this->messages()->create([
            'user_id' => $user->id,
            'type' => MessageTypeEnum::Join,
            'body' => 'chegou no servidor!',
        ]), ['channel_id' => $this->id, 'user_id' => $user->id]);

        $message->setRelation('user', $user);

        self::broadcast(new MessageSent($message));
    }

    /**
     * O token vale 60 s e leva o que a pessoa pode fazer na sala; o SFU só confere.
     *
     * @throws Throwable
     */
    public function voiceToken(User $user, string $ip, ?string $userAgent, SfuClient $sfu): string
    {
        throw_if($this->type !== ChannelTypeEnum::Voice, ForbiddenException::class, 'Este canal não é de voz.');

        $member = $this->memberOrFail($user);
        $member->authorize(PermissionEnum::ViewChannel, $this);
        $member->authorize(PermissionEnum::Connect, $this);

        // Quem pede o token para reconectar ainda consta na presença (a carência do SFU), e não
        // conta contra o limite: sem isto, cair da rede num canal cheio impedia de voltar.
        throw_if(! is_null($this->user_limit) && count(array_filter($sfu->peers($this, fresh: true), fn (array $peer): bool => $peer['sub'] !== $user->subject())) >= $this->user_limit, ForbiddenException::class, 'O canal está cheio.');

        $can = [];

        if ($member->can(PermissionEnum::Speak, $this) && ! $member->server_mute) {
            $can[] = 'speak';
        }

        if ($member->can(PermissionEnum::Stream, $this)) {
            $can[] = 'stream';
        }

        if ($member->can(PermissionEnum::Video, $this)) {
            $can[] = 'video';
        }

        ChannelAccess::open($this, $user, $ip, $userAgent, null, now()->toImmutable());

        return $sfu->token($this, $user, $can);
    }

    /**
     * @throws ForbiddenException
     */
    public function disconnect(User $actor, User $target, SfuClient $sfu): void
    {
        $me = $this->memberOrFail($actor);
        $me->authorize(PermissionEnum::MoveMembers, $this);
        $me->authorizeOutranks($this->memberOrFail($target));

        $sfu->kick($this, $target->subject());
    }

    /**
     * O histórico do canal vai para `channel_audits`, não para a tabela do pacote de
     * auditoria: lá a coluna do id é numérica e a daqui é um ULID de 26 letras.
     */
    protected static function booted(): void
    {
        self::created(fn (self $channel) => ChannelAudit::record($channel, 'created', null, $channel->attributesToArray()));
        self::updated(fn (self $channel) => ChannelAudit::record($channel, 'updated', $channel->getOriginal(), $channel->getChanges()));
        self::deleted(fn (self $channel) => ChannelAudit::record($channel, 'deleted', $channel->attributesToArray(), null));
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'type' => ChannelTypeEnum::class,
            'position' => 'integer',
            'user_limit' => 'integer',
        ];
    }

    /**
     * @throws ForbiddenException|ValidationException
     */
    private function authorizeOverwriteTarget(ServerMember $me, OverwriteTargetEnum $type, int $targetId): void
    {
        if ($type === OverwriteTargetEnum::Member && $this->server->members()->where('user_id', $targetId)->exists()) {
            return;
        }

        $role = $type === OverwriteTargetEnum::Role ? $this->server->roles()->find($targetId) : null;

        if (is_null($role)) {
            throw ValidationException::withMessages(['id' => 'Esse cargo ou membro não existe neste servidor.']);
        }

        $me->authorizeAbove($role);
    }
}
