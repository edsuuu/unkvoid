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
use Illuminate\Support\Str;
use Livewire\Attributes\Computed;
use Livewire\Attributes\On;
use Livewire\Component;
use Throwable;

final class Shell extends Component
{
    public ?string $serverId = null;

    public ?string $channelId = null;

    public string $draft = '';

    public string $newServerName = '';

    public bool $showMembers = true;

    public ?string $renamingChannelId = null;

    public string $renamedChannel = '';

    public bool $showAllMembers = false;

    public string $memberSearch = '';

    public bool $creatingServer = false;

    public bool $editingServer = false;

    public ?string $creatingChannelType = null;

    public string $newChannelName = '';

    public string $serverName = '';

    public function mount(?string $server = null, ?string $channel = null): void
    {
        $this->serverId = $server ?? $this->serverId;
        $this->channelId = $channel ?? $this->channelId;

        $this->ensureSelection();
    }

    #[On('open-server')]
    public function openServer(string $serverId): void
    {
        if ($this->servers->contains('id', $serverId)) {
            $this->selectServer($serverId);
        }
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
            $this->dispatch('stage-changed', stage: 'text');

            return;
        }

        // The URL stores the server, so F5 opens in the right place without needing
        // to rebuild navigation in JavaScript.
        $this->syncUrl();
        $this->dispatch('voice-join', channelId: $channelId, channelName: $channel->name);
    }

    public function createServer(): void
    {
        $validated = $this->validate(['newServerName' => ['required', 'string', 'min:2', 'max:60']]);

        try {
            $server = app(CreateServer::class)->handle(Auth::user(), $validated['newServerName']);
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] server creation failed', ['exception' => $exception]);
            $this->dispatch('toast', variant: 'error', text: __('Unable to create the server.'));

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
            Log::channel('servers')->error('[ERROR] failed to save the message', ['exception' => $exception]);
            $this->dispatch('toast', variant: 'error', text: __('Message not sent.'));

            return;
        }

        $this->draft = '';
        unset($this->messages);
    }

    /**
     * Stops the broadcast. The person remains in the call and chat.
     */
    public function stopBroadcast(string $userId): void
    {
        if (! $this->viewerMember?->canModerate()) {
            return;
        }

        $this->dispatch('voice-stop-broadcast', userId: $userId);
    }

    /**
     * Removes the person from the voice call. They remain a server and chat member.
     */
    public function disconnectFromVoice(string $userId): void
    {
        if (! $this->viewerMember?->canModerate()) {
            return;
        }

        $this->dispatch('voice-disconnect', userId: $userId);
    }

    /**
     * Removes someone from the server. This is the only destructive action of the three.
     */
    public function removeMember(string $memberId): void
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
        $this->dispatch('voice-disconnect', userId: $userId);
        $this->dispatch('toast', variant: 'success', text: __('Member removed from the server.'));
    }

    public function startChannel(string $type): void
    {
        if (! in_array($type, ['text', 'voice'], true) || ! $this->viewerMember?->canModerate()) {
            return;
        }

        $this->newChannelName = '';
        $this->creatingChannelType = $type;
    }

    public function createChannel(): void
    {
        if (! $this->viewerMember?->canModerate() || ! $this->creatingChannelType) {
            return;
        }

        $validated = $this->validate(['newChannelName' => ['required', 'string', 'min:1', 'max:40']]);

        $type = $this->creatingChannelType;
        $name = $type === 'text'
            ? Str::slug($validated['newChannelName']) ?: 'canal'
            : trim($validated['newChannelName']);

        try {
            $channel = Channel::create([
                'server_id' => $this->serverId,
                'name' => $name,
                'type' => $type,
                'position' => $this->channels->where('type', $type)->count(),
            ]);
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] failed to create channel', ['exception' => $exception]);
            $this->dispatch('toast', variant: 'error', text: __('Unable to create the channel.'));

            return;
        }

        $this->creatingChannelType = null;
        $this->newChannelName = '';
        unset($this->channels);

        if ($type === 'text') {
            $this->selectChannel($channel->id);
        }

        $this->dispatch('toast', variant: 'success', text: __('Canal criado.'));
    }

    public function deleteChannel(string $channelId): void
    {
        if (! $this->viewerMember?->canModerate() || $this->channels->count() <= 1) {
            return;
        }

        $channel = $this->channels->firstWhere('id', $channelId);

        if (! $channel) {
            return;
        }

        $channel->delete();
        unset($this->channels);

        if ($this->channelId === $channelId) {
            $this->channelId = null;
            $this->ensureSelection();
        }

        $this->dispatch('toast', variant: 'success', text: __('Channel deleted.'));
    }

    public function startRenameChannel(string $channelId): void
    {
        if (! $this->viewerMember?->canModerate()) {
            return;
        }

        $channel = $this->channels->firstWhere('id', $channelId);

        if (! $channel) {
            return;
        }

        $this->renamingChannelId = $channelId;
        $this->renamedChannel = $channel->name;
    }

    public function renameChannel(): void
    {
        if (! $this->viewerMember?->canModerate() || ! $this->renamingChannelId) {
            return;
        }

        $validated = $this->validate(['renamedChannel' => ['required', 'string', 'min:1', 'max:40']]);
        $channel = $this->channels->firstWhere('id', $this->renamingChannelId);

        if (! $channel) {
            return;
        }

        $channel->update([
            'name' => $channel->type === 'text'
                ? (Str::slug($validated['renamedChannel']) ?: 'canal')
                : trim($validated['renamedChannel']),
        ]);

        $this->renamingChannelId = null;
        unset($this->channels);
        $this->dispatch('toast', variant: 'success', text: __('Canal renomeado.'));
    }

    public function openServerSettings(): void
    {
        $this->serverName = $this->currentServer?->name ?? '';
        $this->editingServer = true;
    }

    public function updateServer(): void
    {
        if ($this->viewerMember?->role !== 'owner') {
            return;
        }

        $validated = $this->validate(['serverName' => ['required', 'string', 'min:2', 'max:60']]);

        try {
            $this->currentServer->update(['name' => $validated['serverName']]);
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] failed to rename server', ['exception' => $exception]);
            $this->dispatch('toast', variant: 'error', text: __('Unable to rename.'));

            return;
        }

        unset($this->servers, $this->currentServer);
        $this->editingServer = false;
        $this->dispatch('toast', variant: 'success', text: __('Servidor renomeado.'));
    }

    public function deleteServer(): void
    {
        if ($this->viewerMember?->role !== 'owner') {
            return;
        }

        try {
            $this->currentServer->delete();
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] failed to delete server', ['exception' => $exception]);
            $this->dispatch('toast', variant: 'error', text: __('Unable to delete.'));

            return;
        }

        $this->editingServer = false;
        $this->serverId = null;
        $this->channelId = null;
        $this->ensureSelection();
        $this->dispatch('toast', variant: 'success', text: __('Server deleted.'));
    }

    public function toggleMembers(): void
    {
        $this->showMembers = ! $this->showMembers;
    }

    public function goHome(): void
    {
        $this->serverId = null;
        $this->channelId = null;
        $this->ensureSelection();
        $this->dispatch('url-changed', url: route('app', absolute: false));
        $this->dispatch('stage-changed', stage: 'text');
    }

    #[Computed]
    public function searchedMembers(): Collection
    {
        $term = mb_strtolower(trim($this->memberSearch));

        if ($term === '') {
            return $this->members;
        }

        return $this->members->filter(
            fn (ServerMember $member): bool => str_contains(mb_strtolower($member->user->displayName()), $term)
        )->values();
    }

    /**
     * Rewrites the URL without navigating. Navigating would drop the voice call, which lives
     * in JavaScript and does not survive a page change.
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

    /**
     * Does not choose a server automatically: /channels is the initial panel. It only ensures that, with
     * a selected server, and an open text channel.
     */
    private function ensureSelection(): void
    {
        unset($this->servers, $this->currentServer, $this->channels, $this->members);

        if (! $this->currentServer) {
            $this->serverId = null;
            $this->channelId = null;
            unset($this->currentServer, $this->channels, $this->members);

            return;
        }

        if ($this->currentChannel) {
            return;
        }

        $this->channelId = $this->channels->firstWhere('type', 'text')?->id;
    }
}
