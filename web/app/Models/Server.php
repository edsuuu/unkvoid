<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\ChannelTypeEnum;
use App\Enums\OverwriteTargetEnum;
use App\Enums\PermissionEnum;
use App\Events\MemberRemoved;
use App\Events\ServerUpdated;
use App\Exceptions\ForbiddenException;
use App\Models\Concerns\LogsFailedWrites;
use App\Services\Sfu\SfuClient;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Collection;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Support\Str;
use Illuminate\Validation\ValidationException;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;
use Throwable;

/**
 * @property int $id
 * @property string $name
 * @property int $owner_id
 * @property string $invite_code
 * @property ?CarbonImmutable $last_accessed_at só em `listFor`
 * @property-read Collection<int, ServerRole> $roles
 * @property-read Collection<int, ServerMember> $members
 * @property-read Collection<int, Channel> $channels
 * @property-read Collection<int, ServerBan> $bans
 */
#[Fillable(['name', 'owner_id', 'invite_code'])]
final class Server extends Model implements Auditable
{
    use AuditableTrait;
    use LogsFailedWrites;

    /**
     * Um servidor nasce com o @everyone, um canal de texto, um de voz e o dono dentro.
     *
     * @throws Throwable
     */
    public static function createFor(User $owner, string $name): self
    {
        return self::write('falha ao criar o servidor', function () use ($owner, $name): self {
            $server = self::query()->create([
                'name' => $name,
                'owner_id' => $owner->id,
                'invite_code' => self::newInviteCode(),
            ]);

            $server->roles()->create([
                'name' => '@everyone',
                'position' => 0,
                'permissions' => PermissionEnum::everyoneDefault(),
                'is_everyone' => true,
            ]);
            $server->channels()->create(['name' => 'geral', 'type' => ChannelTypeEnum::Text, 'position' => 0]);
            $server->channels()->create(['name' => 'Geral', 'type' => ChannelTypeEnum::Voice, 'position' => 1]);
            $server->members()->create(['user_id' => $owner->id, 'joined_at' => now()]);

            return $server;
        }, ['owner_id' => $owner->id]);
    }

    /**
     * @throws Throwable
     */
    public static function joinByInvite(User $user, string $code): self
    {
        $server = self::query()->where('invite_code', $code)->firstOrFail();

        throw_if($server->bans()->where('user_id', $user->id)->exists(), ForbiddenException::class, 'Você foi banido deste servidor.');

        self::write('falha ao entrar no servidor', fn () => $server->members()->firstOrCreate(['user_id' => $user->id], ['joined_at' => now()]), ['server_id' => $server->id, 'user_id' => $user->id]);

        self::broadcast(new ServerUpdated($server->id));

        return $server;
    }

    /**
     * Do último acesso da pessoa à voz para o mais antigo; o nunca acessado vai para o fim,
     * pela data em que ela entrou. O fim vem de graça: `NULL` é o menor valor no MySQL e no
     * SQLite, e cai por último na ordem decrescente.
     *
     * @return Collection<int, self>
     */
    public static function listFor(User $user): Collection
    {
        $lastAccess = ChannelAccess::query()
            ->selectRaw('max(channel_accesses.joined_at)')
            ->join('channels', 'channels.id', '=', 'channel_accesses.channel_id')
            ->whereColumn('channels.server_id', 'servers.id')
            ->where('channel_accesses.user_id', $user->id);

        return self::query()
            ->select('servers.*')
            ->addSelect(['last_accessed_at' => $lastAccess])
            ->join('server_members', 'server_members.server_id', '=', 'servers.id')
            ->where('server_members.user_id', $user->id)
            ->withCasts(['last_accessed_at' => 'datetime'])
            ->orderByDesc('last_accessed_at')
            ->orderByDesc('server_members.joined_at')
            ->orderBy('servers.id')
            ->get();
    }

    /**
     * @return HasMany<ServerRole, $this>
     */
    public function roles(): HasMany
    {
        return $this->hasMany(ServerRole::class);
    }

    /**
     * @return HasMany<ServerMember, $this>
     */
    public function members(): HasMany
    {
        return $this->hasMany(ServerMember::class);
    }

    /**
     * @return HasMany<Channel, $this>
     */
    public function channels(): HasMany
    {
        return $this->hasMany(Channel::class);
    }

    /**
     * @return HasMany<ServerBan, $this>
     */
    public function bans(): HasMany
    {
        return $this->hasMany(ServerBan::class);
    }

    public function everyoneRole(): ServerRole
    {
        return $this->roles->firstOrFail('is_everyone', true);
    }

    public function memberOf(User $user): ?ServerMember
    {
        $member = $this->members()->where('user_id', $user->id)->first();

        $member?->setRelation('server', $this);

        return $member;
    }

    /**
     * @throws ForbiddenException
     */
    public function memberOrFail(User $user): ServerMember
    {
        $member = $this->memberOf($user);

        throw_if(is_null($member), ForbiddenException::class, 'Você não é membro deste servidor.');

        return $member;
    }

    /**
     * @throws Throwable
     */
    public function rename(User $actor, string $name): void
    {
        $this->memberOrFail($actor)->authorize(PermissionEnum::ManageServer);

        self::write('falha ao renomear o servidor', fn () => $this->update(['name' => $name]), ['server_id' => $this->id]);

        self::broadcast(new ServerUpdated($this->id));
    }

    /**
     * @throws Throwable
     */
    public function destroyBy(User $actor): void
    {
        throw_if($this->owner_id !== $actor->id, ForbiddenException::class, 'Só o dono apaga o servidor.');

        self::write('falha ao apagar o servidor', fn () => $this->delete(), ['server_id' => $this->id]);
    }

    /**
     * @throws Throwable
     */
    public function regenerateInvite(User $actor): string
    {
        $this->memberOrFail($actor)->authorize(PermissionEnum::CreateInvite);

        $code = self::newInviteCode();

        self::write('falha ao gerar o convite', fn () => $this->update(['invite_code' => $code]), ['server_id' => $this->id]);

        return $code;
    }

    /**
     * @throws Throwable
     */
    public function leave(User $user, SfuClient $sfu): void
    {
        throw_if($this->owner_id === $user->id, ForbiddenException::class, 'O dono não sai do servidor: apague-o ou passe-o adiante.');

        $member = $this->memberOrFail($user);

        self::write('falha ao sair do servidor', function () use ($member, $user): void {
            $this->removeMemberOverwrites($user);
            $member->delete();
        }, ['server_id' => $this->id, 'user_id' => $user->id]);

        $this->dropFromVoice($user, $sfu);

        self::broadcast(new ServerUpdated($this->id));
    }

    /**
     * @throws Throwable
     */
    public function kick(User $actor, User $target, SfuClient $sfu): void
    {
        $me = $this->memberOrFail($actor);
        $me->authorize(PermissionEnum::KickMembers);

        $other = $this->memberOrFail($target);
        $me->authorizeOutranks($other);

        self::write('falha ao expulsar o membro', function () use ($other, $target): void {
            $this->removeMemberOverwrites($target);
            $other->delete();
        }, ['server_id' => $this->id, 'user_id' => $target->id]);

        $this->dropFromVoice($target, $sfu);

        self::broadcast(new MemberRemoved($this->id, $target->id, 'kicked'));
        self::broadcast(new ServerUpdated($this->id));
    }

    /**
     * @throws Throwable
     */
    public function ban(User $actor, User $target, ?string $reason, SfuClient $sfu): ServerBan
    {
        $me = $this->memberOrFail($actor);
        $me->authorize(PermissionEnum::BanMembers);

        $other = $this->memberOf($target);

        if (! is_null($other)) {
            $me->authorizeOutranks($other);
        }

        $ban = self::write('falha ao banir', function () use ($actor, $target, $reason, $other): ServerBan {
            $ban = $this->bans()->firstOrCreate(['user_id' => $target->id], ['banned_by' => $actor->id, 'reason' => $reason]);
            $this->removeMemberOverwrites($target);
            $other?->delete();

            return $ban;
        }, ['server_id' => $this->id, 'user_id' => $target->id]);

        $this->dropFromVoice($target, $sfu);

        self::broadcast(new MemberRemoved($this->id, $target->id, 'banned'));
        self::broadcast(new ServerUpdated($this->id));

        return $ban->load('user');
    }

    /**
     * @throws Throwable
     */
    public function unban(User $actor, User $target): void
    {
        $this->memberOrFail($actor)->authorize(PermissionEnum::BanMembers);

        self::write('falha ao remover o banimento', fn () => $this->bans()->where('user_id', $target->id)->delete(), ['server_id' => $this->id, 'user_id' => $target->id]);

        self::broadcast(new ServerUpdated($this->id));
    }

    /**
     * @param  array{nickname?: ?string, role_ids?: array<int, int>, server_mute?: bool, server_deaf?: bool}  $changes
     *
     * @throws Throwable
     */
    public function updateMember(User $actor, User $target, array $changes, SfuClient $sfu): ServerMember
    {
        $me = $this->memberOrFail($actor);
        $other = $this->memberOrFail($target);
        $self = $actor->id === $target->id;
        $roles = isset($changes['role_ids']) ? $this->roles()->whereIn('id', $changes['role_ids'])->where('is_everyone', false)->get() : new Collection;

        if (array_key_exists('nickname', $changes) && ! $self) {
            $me->authorize(PermissionEnum::ManageServer);
            $me->authorizeOutranks($other);
        }

        if (isset($changes['role_ids'])) {
            $me->authorize(PermissionEnum::ManageRoles);
        }

        if (isset($changes['role_ids']) && ! $self) {
            $me->authorizeOutranks($other);
        }

        foreach ($roles as $role) {
            $me->authorizeAbove($role);

            if ($other->roles->doesntContain($role)) {
                $me->authorizeGrantable($role->permissions);
            }
        }

        if (isset($changes['server_mute'])) {
            $me->authorize(PermissionEnum::MuteMembers);
            $me->authorizeOutranks($other);
        }

        if (isset($changes['server_deaf'])) {
            $me->authorize(PermissionEnum::DeafenMembers);
            $me->authorizeOutranks($other);
        }

        self::write('falha ao alterar o membro', function () use ($other, $changes, $roles): void {
            $other->update(array_intersect_key($changes, ['nickname' => true, 'server_mute' => true, 'server_deaf' => true]));

            if (isset($changes['role_ids'])) {
                $other->auditSync('roles', $roles->modelKeys());
            }
        }, ['server_id' => $this->id, 'user_id' => $target->id]);

        if (isset($changes['server_mute'])) {
            $channel = $this->voiceChannelOf($target, $sfu);

            if (! is_null($channel)) {
                $sfu->mute($channel, $target->subject(), $changes['server_mute']);
            }
        }

        self::broadcast(new ServerUpdated($this->id));

        return $other->refresh();
    }

    /**
     * O dono não tem cargo: o cargo novo dele vai para o topo da lista. O dos outros nasce
     * logo abaixo do próprio, nunca abaixo de 1.
     *
     * @throws Throwable
     */
    public function createRole(User $actor, string $name, ?string $color, int $permissions): ServerRole
    {
        $me = $this->memberOrFail($actor);
        $me->authorize(PermissionEnum::ManageRoles);
        $me->authorizeGrantable($permissions);

        $position = $me->isOwner() ? $this->nextPosition($this->roles) : max($me->topPosition() - 1, 1);

        $role = self::write('falha ao criar o cargo', fn (): ServerRole => $this->roles()->create([
            'name' => $name,
            'color' => $color,
            'position' => $position,
            'permissions' => $permissions,
        ]), ['server_id' => $this->id]);

        self::broadcast(new ServerUpdated($this->id));

        return $role;
    }

    /**
     * @throws Throwable
     */
    public function createChannel(User $actor, string $name, ChannelTypeEnum $type, ?string $topic, ?int $userLimit): Channel
    {
        $this->memberOrFail($actor)->authorize(PermissionEnum::ManageChannels);

        if ($type === ChannelTypeEnum::Text && ! is_null($userLimit)) {
            throw ValidationException::withMessages(['user_limit' => 'Só canal de voz tem limite de pessoas.']);
        }

        $channel = self::write('falha ao criar o canal', fn (): Channel => $this->channels()->create([
            'name' => $name,
            'type' => $type,
            'topic' => $topic,
            'user_limit' => $userLimit,
            'position' => $this->nextPosition($this->channels),
        ]), ['server_id' => $this->id]);

        self::broadcast(new ServerUpdated($this->id));

        return $channel;
    }

    public function voiceChannelOf(User $user, SfuClient $sfu): ?Channel
    {
        foreach ($this->channels as $channel) {
            if ($channel->type !== ChannelTypeEnum::Voice) {
                continue;
            }

            foreach ($sfu->peers($channel) as $peer) {
                if ($peer['sub'] === $user->subject()) {
                    return $channel;
                }
            }
        }

        return null;
    }

    private static function newInviteCode(): string
    {
        return mb_strtolower(Str::random(10));
    }

    /**
     * @param  Collection<int, ServerRole>|Collection<int, Channel>  $items
     */
    private function nextPosition(Collection $items): int
    {
        $top = 0;

        foreach ($items as $item) {
            $top = max($top, $item->position);
        }

        return $top + 1;
    }

    private function removeMemberOverwrites(User $user): void
    {
        ChannelOverwrite::query()
            ->whereIn('channel_id', $this->channels()->select('id'))
            ->where('target_type', OverwriteTargetEnum::Member)
            ->where('target_id', $user->id)
            ->delete();
    }

    private function dropFromVoice(User $user, SfuClient $sfu): void
    {
        $channel = $this->voiceChannelOf($user, $sfu);

        if (is_null($channel)) {
            return;
        }

        $sfu->kick($channel, $user->subject());
    }
}
