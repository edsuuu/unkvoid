<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\OverwriteTargetEnum;
use App\Enums\PermissionEnum;
use App\Exceptions\ForbiddenException;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Collection;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;

/**
 * @property int $id
 * @property int $server_id
 * @property int $user_id
 * @property ?string $nickname
 * @property bool $server_mute
 * @property bool $server_deaf
 * @property CarbonImmutable $joined_at
 * @property-read Server $server
 * @property-read User $user
 * @property-read Collection<int, ServerRole> $roles
 */
#[Fillable(['server_id', 'user_id', 'nickname', 'server_mute', 'server_deaf', 'joined_at'])]
final class ServerMember extends Model implements Auditable
{
    use AuditableTrait;

    /**
     * @return BelongsTo<Server, $this>
     */
    public function server(): BelongsTo
    {
        return $this->belongsTo(Server::class);
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    /**
     * @return BelongsToMany<ServerRole, $this>
     */
    public function roles(): BelongsToMany
    {
        return $this->belongsToMany(ServerRole::class, 'member_roles', 'server_member_id', 'role_id');
    }

    public function isOwner(): bool
    {
        return $this->server->owner_id === $this->user_id;
    }

    /**
     * As permissões efetivas, na ordem do Discord: dono e ADMINISTRATOR têm tudo; a base
     * é @everyone com os cargos por cima; num canal, as sobrescritas entram na ordem
     *
     * @everyone, cargos agregados, membro.
     */
    public function permissions(?Channel $channel = null): int
    {
        $everyone = $this->server->everyoneRole();
        $base = $everyone->permissions;

        foreach ($this->roles as $role) {
            $base |= $role->permissions;
        }

        if ($this->isOwner() || ($base & PermissionEnum::Administrator->value) !== 0) {
            return PermissionEnum::all();
        }

        if (is_null($channel)) {
            return $base;
        }

        $roleIds = $this->roles->pluck('id')->all();
        $everyoneAllow = 0;
        $everyoneDeny = 0;
        $rolesAllow = 0;
        $rolesDeny = 0;
        $memberAllow = 0;
        $memberDeny = 0;

        foreach ($channel->overwrites as $overwrite) {
            if ($overwrite->target_type === OverwriteTargetEnum::Member && $overwrite->target_id !== $this->user_id) {
                continue;
            }

            if ($overwrite->target_type === OverwriteTargetEnum::Member) {
                $memberAllow = $overwrite->allow;
                $memberDeny = $overwrite->deny;

                continue;
            }

            if ($overwrite->target_id === $everyone->id) {
                $everyoneAllow = $overwrite->allow;
                $everyoneDeny = $overwrite->deny;

                continue;
            }

            if (in_array($overwrite->target_id, $roleIds, true)) {
                $rolesAllow |= $overwrite->allow;
                $rolesDeny |= $overwrite->deny;
            }
        }

        $base = ($base & ~$everyoneDeny) | $everyoneAllow;
        $base = ($base & ~$rolesDeny) | $rolesAllow;

        return ($base & ~$memberDeny) | $memberAllow;
    }

    public function can(PermissionEnum $permission, ?Channel $channel = null): bool
    {
        return ($this->permissions($channel) & $permission->value) !== 0;
    }

    /**
     * @throws ForbiddenException
     */
    public function authorize(PermissionEnum $permission, ?Channel $channel = null): void
    {
        throw_unless($this->can($permission, $channel), ForbiddenException::class, 'Você não tem permissão para isso.');
    }

    public function topPosition(): int
    {
        if ($this->isOwner()) {
            return PHP_INT_MAX;
        }

        $top = 0;

        foreach ($this->roles as $role) {
            $top = max($top, $role->position);
        }

        return $top;
    }

    public function outranks(self $other): bool
    {
        if ($other->isOwner()) {
            return false;
        }

        if ($this->isOwner()) {
            return true;
        }

        return $this->topPosition() > $other->topPosition();
    }

    /**
     * @throws ForbiddenException
     */
    public function authorizeOutranks(self $other): void
    {
        throw_unless($this->outranks($other), ForbiddenException::class, 'Essa pessoa tem um cargo igual ou acima do seu.');
    }

    /**
     * @throws ForbiddenException
     */
    public function authorizeAbove(ServerRole $role): void
    {
        throw_if($role->position >= $this->topPosition(), ForbiddenException::class, 'Esse cargo está igual ou acima do seu.');
    }

    /**
     * Ninguém dá a um cargo o que não tem: é o que impede alguém com MANAGE_ROLES de se
     * tornar ADMINISTRATOR.
     *
     * @throws ForbiddenException
     */
    public function authorizeGrantable(int $permissions): void
    {
        throw_if(($permissions & ~$this->permissions()) !== 0, ForbiddenException::class, 'Você não pode dar permissões que não tem.');
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'server_mute' => 'boolean',
            'server_deaf' => 'boolean',
            'joined_at' => 'datetime',
        ];
    }
}
