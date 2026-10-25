<?php

declare(strict_types=1);

namespace App\Livewire\Invite;

use App\Models\Server;
use App\Models\ServerInvite;
use Illuminate\View\View;
use Livewire\Attributes\Locked;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Convite')]
final class Show extends Component
{
    #[Locked]
    public string $code = '';

    public function render(): View
    {
        // As mesmas regras da entrada pelo app (`Server::joinByInvite`): o convite da pessoa
        // primeiro, depois o código único do servidor. Do servidor só sai o que o convite já
        // mostraria a quem o recebeu; convite inválido não diz se o servidor existe.
        $invite = ServerInvite::query()->with(['server', 'user'])->where('code', $this->code)->first();
        $server = $invite->server ?? Server::query()->where('invite_code', $this->code)->first();

        $state = match (true) {
            is_null($server) => 'missing',
            ! is_null($invite) && $invite->spent() => 'spent',
            ! is_null($server->invites_paused_at) => 'paused',
            default => 'valid',
        };

        $valid = $state === 'valid' && ! is_null($server);

        return view('livewire.invite.show', [
            'state' => $state,
            'serverName' => $valid ? $server->name : null,
            'iconUrl' => $valid ? $server->iconUrl() : null,
            'inviter' => $valid ? $invite?->user?->name : null,
            'members' => $valid ? $server->members()->count() : 0,
        ]);
    }
}
