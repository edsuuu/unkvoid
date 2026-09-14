<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Enums\ChannelTypeEnum;
use App\Enums\PermissionEnum;
use App\Models\Server;
use App\Models\ServerMember;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * A árvore inteira de um servidor, do ponto de vista de quem pediu: só os canais que
 * ela vê, e o convite, as sobrescritas e os banimentos só com a permissão certa.
 */
final class ServerTreeResource extends JsonResource
{
    private const int OWNER_POSITION = 2147483647;

    public function __construct(
        private readonly Server $server,
        private readonly ServerMember $me,
        private readonly SfuClient $sfu,
    ) {
        parent::__construct($server);
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        $this->server->load(['roles', 'members.user', 'members.roles', 'channels.overwrites', 'bans.user']);
        $this->me->setRelation('server', $this->server);

        $canInvite = $this->me->can(PermissionEnum::CreateInvite);
        $canManageRoles = $this->me->can(PermissionEnum::ManageRoles);
        $canBan = $this->me->can(PermissionEnum::BanMembers);

        $channels = [];
        $voice = [];

        foreach ($this->server->channels->sortBy('position') as $channel) {
            if (! $this->me->can(PermissionEnum::ViewChannel, $channel)) {
                continue;
            }

            $entry = new ChannelResource($channel)->resolve();
            $entry['permissions'] = $this->me->permissions($channel);
            $entry['overwrites'] = $canManageRoles ? OverwriteResource::collection($channel->overwrites)->resolve() : [];
            $channels[] = $entry;

            if ($channel->type !== ChannelTypeEnum::Voice) {
                continue;
            }

            $voice[$channel->id] = [];

            foreach ($this->sfu->peers($channel) as $peer) {
                $voice[$channel->id][] = [
                    'user_id' => User::fromSubject($peer['sub']),
                    'name' => $peer['name'],
                    'sources' => $peer['sources'],
                ];
            }
        }

        foreach ($this->server->members as $member) {
            $member->setRelation('server', $this->server);
        }

        return [
            'id' => $this->server->id,
            'name' => $this->server->name,
            'owner_id' => $this->server->owner_id,
            'invite_code' => $canInvite ? $this->server->invite_code : null,
            'me' => [
                'user_id' => $this->me->user_id,
                'permissions' => $this->me->permissions(),
                'top_position' => $this->me->isOwner() ? self::OWNER_POSITION : $this->me->topPosition(),
            ],
            'roles' => RoleResource::collection($this->server->roles->sortBy('position')->values())->resolve(),
            'channels' => $channels,
            'members' => MemberResource::collection($this->server->members)->resolve(),
            'voice' => (object) $voice,
            'bans' => $canBan ? BanResource::collection($this->server->bans)->resolve() : [],
        ];
    }
}
