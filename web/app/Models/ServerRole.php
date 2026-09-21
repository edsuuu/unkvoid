<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\OverwriteTargetEnum;
use App\Enums\PermissionEnum;
use App\Events\ServerUpdated;
use App\Exceptions\ForbiddenException;
use App\Models\Concerns\LogsFailedWrites;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Validation\ValidationException;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;
use Throwable;

/**
 * @property int $id
 * @property int $server_id
 * @property string $name
 * @property ?string $color
 * @property int $position
 * @property int $permissions
 * @property bool $is_everyone
 * @property-read Server $server
 */
#[Fillable(['server_id', 'name', 'color', 'position', 'permissions', 'is_everyone'])]
final class ServerRole extends Model implements Auditable
{
    use AuditableTrait;
    use LogsFailedWrites;

    /**
     * @return BelongsTo<Server, $this>
     */
    public function server(): BelongsTo
    {
        return $this->belongsTo(Server::class);
    }

    /**
     * @param  array{name?: string, color?: ?string, permissions?: int, position?: int}  $changes
     *
     * @throws Throwable
     */
    public function change(User $actor, array $changes): void
    {
        $member = $this->server->memberOrFail($actor);
        $member->authorize(PermissionEnum::ManageRoles);
        $member->authorizeGrantable($changes['permissions'] ?? 0);

        if ($this->is_everyone) {
            $changes = array_intersect_key($changes, ['permissions' => true]);
        } else {
            $member->authorizeAbove($this);
        }

        throw_if(isset($changes['position']) && $changes['position'] >= $member->topPosition(), ForbiddenException::class, 'Essa posição está igual ou acima do seu cargo.');

        self::write('falha ao alterar o cargo', fn () => $this->update($changes), ['role_id' => $this->id]);

        self::publish(new ServerUpdated($this->server_id));
    }

    /**
     * @throws Throwable
     */
    public function remove(User $actor): void
    {
        if ($this->is_everyone) {
            throw ValidationException::withMessages(['role' => 'O cargo @everyone não pode ser apagado.']);
        }

        $member = $this->server->memberOrFail($actor);
        $member->authorize(PermissionEnum::ManageRoles);
        $member->authorizeAbove($this);

        self::write('falha ao apagar o cargo', function (): void {
            ChannelOverwrite::query()
                ->whereIn('channel_id', $this->server->channels()->select('id'))
                ->where('target_type', OverwriteTargetEnum::Role)
                ->where('target_id', $this->id)
                ->delete();
            $this->delete();
        }, ['role_id' => $this->id]);

        self::publish(new ServerUpdated($this->server_id));
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'position' => 'integer',
            'permissions' => 'integer',
            'is_everyone' => 'boolean',
        ];
    }
}
