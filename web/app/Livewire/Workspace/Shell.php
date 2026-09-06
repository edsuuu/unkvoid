<?php

declare(strict_types=1);

namespace App\Livewire\Workspace;

use App\Actions\Servers\CreateServer;
use App\Models\Channel;
use App\Models\Message;
use App\Models\Server;
use App\Models\ServerMember;
use Illuminate\Contracts\View\View;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Livewire\Attributes\Computed;
use Livewire\Component;
use Throwable;

final class Shell extends Component
{
    public ?string $serverId = null;

    public ?string $channelId = null;

    public string $draft = '';

    public string $newServerName = '';

    public bool $showMembers = true;

    public bool $creatingServer = false;

    public function mount(?string $server = null, ?string $channel = null): void
    {
        $this->serverId = $server ?? $this->serverId;
        $this->channelId = $channel ?? $this->channelId;

        $this->ensureSelection();
    }

    public function selectServer(string $serverId): void
    {
        $this->serverId = $serverId;
        $this->channelId = null;
        $this->ensureSelection();
        $this->syncUrl();
    }

    public function selectChannel(string $channelId): void
    {
        $channel = $this->channels->firstWhere('id', $channelId);

        if (! $channel) {
            return;
        }

        if ($channel->type === 'text') {
            $this->channelId = $channelId;
            $this->syncUrl();

            return;
        }

        $this->dispatch('voice-join', channelId: $channelId, channelName: $channel->name);
    }

    public function createServer(): void
    {
        $validated = $this->validate(['newServerName' => ['required', 'string', 'min:2', 'max:60']]);

        try {
            $server = app(CreateServer::class)->handle(Auth::user(), $validated['newServerName']);
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERRO] criação de servidor falhou', ['exception' => $exception]);
            $this->dispatch('toast', variant: 'error', text: __('Não foi possível criar o servidor.'));

            return;
        }

        $this->newServerName = '';
        $this->creatingServer = false;
        $this->selectServer($server->id);
        $this->dispatch('toast', variant: 'success', text: __('Servidor criado.'));
    }

    public function sendMessage(): void
    {
        $validated = $this->validate(['draft' => ['required', 'string', 'max:2000']]);

        if (! $this->currentChannel || $this->currentChannel->type !== 'text') {
            return;
        }

        try {
            Message::create([
                'channel_id' => $this->currentChannel->id,
                'user_id' => Auth::id(),
                'content' => $validated['draft'],
            ]);
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERRO] falha ao gravar mensagem', ['exception' => $exception]);
            $this->dispatch('toast', variant: 'error', text: __('Mensagem não enviada.'));

            return;
        }

        $this->draft = '';
        unset($this->messages);
    }

    public function kickMember(string $memberId): void
    {
        if (! $this->viewerMember?->canModerate()) {
            return;
        }

        $member = ServerMember::where('server_id', $this->serverId)->find($memberId);

        if (! $member || $member->role === 'owner') {
            return;
        }

        $userId = $member->user_id;
        $member->delete();

        unset($this->members);
        $this->dispatch('voice-kick', userId: $userId);
        $this->dispatch('toast', variant: 'success', text: __('Membro removido do servidor.'));
    }

    public function stopBroadcast(string $userId): void
    {
        if (! $this->viewerMember?->canModerate()) {
            return;
        }

        $this->dispatch('voice-stop-broadcast', userId: $userId);
    }

    public function toggleMembers(): void
    {
        $this->showMembers = ! $this->showMembers;
    }

    /**
     * Reescreve a URL sem navegar. Navegar derrubaria a chamada de voz, que vive
     * em JavaScript e não sobrevive a uma troca de página.
     */
    private function syncUrl(): void
    {
        if (! $this->serverId) {
            return;
        }

        $this->dispatch('url-changed', url: route('channel', [
            'server' => $this->serverId,
            'channel' => $this->channelId,
        ], absolute: false));
    }

    #[Computed]
    public function servers(): Collection
    {
        return Auth::user()->servers()->get();
    }

    #[Computed]
    public function currentServer(): ?Server
    {
        return $this->serverId ? $this->servers->firstWhere('id', $this->serverId) : null;
    }

    #[Computed]
    public function channels(): Collection
    {
        return $this->currentServer?->channels()->get() ?? collect();
    }

    #[Computed]
    public function currentChannel(): ?Channel
    {
        return $this->channelId ? $this->channels->firstWhere('id', $this->channelId) : null;
    }

    #[Computed]
    public function members(): Collection
    {
        return $this->currentServer
            ? $this->currentServer->members()->with('user')->get()
            : collect();
    }

    #[Computed]
    public function viewerMember(): ?ServerMember
    {
        return $this->members->firstWhere('user_id', Auth::id());
    }

    #[Computed]
    public function messages(): Collection
    {
        if (! $this->currentChannel || $this->currentChannel->type !== 'text') {
            return collect();
        }

        return $this->currentChannel->messages()
            ->with('user')
            ->latest('created_at')
            ->limit(60)
            ->get()
            ->reverse()
            ->values();
    }

    #[Computed]
    public function inviteUrl(): ?string
    {
        return $this->currentServer ? route('invite', ['code' => $this->currentServer->invite_code]) : null;
    }

    public function render(): View
    {
        return view('livewire.workspace.shell');
    }

    private function ensureSelection(): void
    {
        unset($this->servers, $this->currentServer, $this->channels, $this->members);

        if (! $this->currentServer) {
            $this->serverId = $this->servers->first()?->id;
            unset($this->currentServer, $this->channels, $this->members);
        }

        if ($this->currentChannel) {
            return;
        }

        $this->channelId = $this->channels->firstWhere('type', 'text')?->id;
    }
}
