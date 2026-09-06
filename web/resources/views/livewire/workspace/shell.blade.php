<div class="flex h-screen w-screen overflow-hidden bg-[#313338] text-[#dbdee1]" x-data="{ invite: false, inCall: false, voiceStatus: '{{ __('Disponível') }}' }"
     x-on:voice-state.window="inCall = $event.detail.inCall"
     x-on:voice-status.window="voiceStatus = $event.detail.text">

    {{-- Trilha de servidores --}}
    <nav class="flex w-[72px] shrink-0 flex-col items-center gap-2 overflow-y-auto bg-[#1e1f22] py-3">
        @foreach ($this->servers as $server)
            <button
                type="button"
                wire:key="server-{{ $server->id }}"
                wire:click="selectServer('{{ $server->id }}')"
                class="group relative flex size-12 items-center justify-center"
                title="{{ $server->name }}"
            >
                <span @class([
                    'absolute -left-3 w-1 rounded-r-full bg-white transition-all',
                    'h-10' => $server->id === $this->serverId,
                    'h-0 group-hover:h-5' => $server->id !== $this->serverId,
                ])></span>

                <span @class([
                    'flex size-12 items-center justify-center text-sm font-semibold transition-all duration-200',
                    'rounded-2xl bg-[#5865f2] text-white' => $server->id === $this->serverId,
                    'rounded-[24px] bg-[#313338] text-[#dbdee1] group-hover:rounded-2xl group-hover:bg-[#5865f2] group-hover:text-white' => $server->id !== $this->serverId,
                ])>{{ $server->initials() }}</span>
            </button>
        @endforeach

        <button
            type="button"
            wire:click="$set('creatingServer', true)"
            class="flex size-12 items-center justify-center rounded-[24px] bg-[#313338] text-2xl text-[#23a55a] transition-all duration-200 hover:rounded-2xl hover:bg-[#23a55a] hover:text-white"
            title="{{ __('Criar servidor') }}"
        >+</button>
    </nav>

    {{-- Sidebar de canais --}}
    <aside class="flex w-60 shrink-0 flex-col bg-[#2b2d31]">
        <header class="flex h-12 shrink-0 items-center justify-between border-b border-[#1f2023] px-4 shadow-sm">
            <span class="truncate font-semibold text-white">{{ $this->currentServer?->name ?? __('Nenhum servidor') }}</span>

            @if ($this->currentServer)
                <button type="button" x-on:click="invite = true" class="text-[#b5bac1] hover:text-white" title="{{ __('Convidar') }}">
                    <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M13.19 8.688a4.5 4.5 0 0 1 1.242 7.244l-4.5 4.5a4.5 4.5 0 0 1-6.364-6.364l1.757-1.757m13.35-.622 1.757-1.757a4.5 4.5 0 0 0-6.364-6.364l-4.5 4.5a4.5 4.5 0 0 0 1.242 7.244"/></svg>
                </button>
            @endif
        </header>

        <div class="flex-1 overflow-y-auto px-2 py-3">
            @if ($this->channels->where('type', 'text')->isNotEmpty())
                <p class="px-2 pb-1 pt-2 text-xs font-bold uppercase tracking-wide text-[#949ba4]">{{ __('Canais de texto') }}</p>

                @foreach ($this->channels->where('type', 'text') as $channel)
                    <button
                        type="button"
                        wire:key="channel-{{ $channel->id }}"
                        wire:click="selectChannel('{{ $channel->id }}')"
                        @class([
                            'group flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-[15px]',
                            'bg-[#404249] text-white' => $channel->id === $this->channelId,
                            'text-[#949ba4] hover:bg-[#35373c] hover:text-[#dbdee1]' => $channel->id !== $this->channelId,
                        ])
                    >
                        <span class="text-xl leading-none text-[#80848e]">#</span>
                        <span class="truncate">{{ $channel->name }}</span>
                    </button>
                @endforeach
            @endif

            @if ($this->channels->where('type', 'voice')->isNotEmpty())
                <p class="px-2 pb-1 pt-4 text-xs font-bold uppercase tracking-wide text-[#949ba4]">{{ __('Canais de voz') }}</p>

                @foreach ($this->channels->where('type', 'voice') as $channel)
                    <div wire:key="voice-{{ $channel->id }}" x-data>
                        <button
                            type="button"
                            wire:click="selectChannel('{{ $channel->id }}')"
                            class="group flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-[15px] text-[#949ba4] hover:bg-[#35373c] hover:text-[#dbdee1]"
                        >
                            <svg class="size-5 shrink-0 text-[#80848e]" fill="currentColor" viewBox="0 0 24 24"><path d="M11.383 3.076A1 1 0 0 1 12 4v16a1 1 0 0 1-1.707.707L5.586 16H3a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1h2.586l4.707-4.707a1 1 0 0 1 1.09-.217zM16.5 7.5a1 1 0 0 1 1.414 0 6 6 0 0 1 0 8.486 1 1 0 1 1-1.414-1.414 4 4 0 0 0 0-5.658 1 1 0 0 1 0-1.414z"/></svg>
                            <span class="truncate">{{ $channel->name }}</span>
                        </button>

                        <div class="ml-6 space-y-0.5" data-voice-members="{{ $channel->id }}"></div>
                    </div>
                @endforeach
            @endif
        </div>

        {{-- Painel do usuário --}}
        <div class="flex h-[52px] shrink-0 items-center gap-2 bg-[#232428] px-2">
            <div class="flex size-8 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-xs font-semibold text-white">
                @if (auth()->user()->avatar_url)
                    <img src="{{ auth()->user()->avatar_url }}" alt="" class="size-8 object-cover">
                @else
                    {{ auth()->user()->initials() }}
                @endif
            </div>

            <div class="min-w-0 flex-1 leading-tight">
                <p class="truncate text-sm font-medium text-white">{{ auth()->user()->displayName() }}</p>
                <p class="truncate text-xs text-[#949ba4]" x-text="voiceStatus"></p>
            </div>

            <button type="button" data-action="toggle-mic" class="rounded p-1.5 text-[#b5bac1] hover:bg-[#35373c] hover:text-white" title="{{ __('Microfone') }}">
                <svg class="size-5" fill="currentColor" viewBox="0 0 24 24"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 1 0-6 0v5a3 3 0 0 0 3 3z"/><path d="M18 11a1 1 0 1 0-2 0 4 4 0 0 1-8 0 1 1 0 1 0-2 0 6 6 0 0 0 5 5.917V19H9a1 1 0 1 0 0 2h6a1 1 0 1 0 0-2h-2v-2.083A6 6 0 0 0 18 11z"/></svg>
            </button>

            <a href="{{ route('profile.edit') }}" wire:navigate class="rounded p-1.5 text-[#b5bac1] hover:bg-[#35373c] hover:text-white" title="{{ __('Configurações') }}">
                <svg class="size-5" fill="none" stroke="currentColor" stroke-width="1.8" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M10.34 3.94c.09-.54.55-.94 1.1-.94h1.12c.55 0 1.01.4 1.1.94l.15.9c.44.16.86.38 1.24.65l.85-.32c.51-.2 1.09 0 1.37.48l.56.97c.28.48.15 1.09-.29 1.42l-.72.54c.04.24.06.48.06.72s-.02.48-.06.72l.72.54c.44.33.57.94.29 1.42l-.56.97c-.28.48-.86.68-1.37.48l-.85-.32c-.38.27-.8.49-1.24.65l-.15.9c-.09.54-.55.94-1.1.94h-1.12c-.55 0-1.01-.4-1.1-.94l-.15-.9a5.7 5.7 0 0 1-1.24-.65l-.85.32c-.51.2-1.09 0-1.37-.48l-.56-.97a1.12 1.12 0 0 1 .29-1.42l.72-.54a5.6 5.6 0 0 1 0-1.44l-.72-.54a1.12 1.12 0 0 1-.29-1.42l.56-.97c.28-.48.86-.68 1.37-.48l.85.32c.38-.27.8-.49 1.24-.65l.15-.9z"/><circle cx="12" cy="12" r="2.5"/></svg>
            </a>
        </div>
    </aside>

    {{-- Conteúdo --}}
    <main class="flex min-w-0 flex-1 flex-col bg-[#313338]">
        <header class="flex h-12 shrink-0 items-center gap-2 border-b border-[#1f2023] px-4 shadow-sm">
            @if ($this->currentChannel)
                <span class="text-xl text-[#80848e]">#</span>
                <span class="font-semibold text-white">{{ $this->currentChannel->name }}</span>
            @else
                <span class="font-semibold text-white">{{ __('Selecione um canal') }}</span>
            @endif

            <span class="flex-1"></span>

            <button
                type="button"
                wire:click="toggleMembers"
                @class(['rounded p-1.5 hover:bg-[#35373c]', 'text-white' => $this->showMembers, 'text-[#b5bac1]' => ! $this->showMembers])
                title="{{ __('Membros') }}"
            >
                <svg class="size-5" fill="currentColor" viewBox="0 0 24 24"><path d="M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zm0 2c-3.3 0-6 1.8-6 4v2h12v-2c0-2.2-2.7-4-6-4zm8-2a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm0 2c-.6 0-1.2.1-1.7.2 1.1.9 1.7 2 1.7 3.3v2h5v-2c0-1.9-2.3-3.5-5-3.5z"/></svg>
            </button>
        </header>

        {{-- Área de voz. wire:ignore é obrigatório: sem ele o Livewire recria este
             trecho a cada render e leva junto os <video> da chamada. --}}
        <section
            wire:ignore
            data-voice-stage
            x-show="inCall"
            x-cloak
            class="flex min-h-0 flex-1 flex-col bg-[#1e1f22]"
        >
            <div data-voice-grid class="grid min-h-0 flex-1 gap-3 overflow-y-auto p-4"></div>

            <div class="flex shrink-0 items-center justify-center gap-2 border-t border-[#1f2023] bg-[#232428] px-4 py-3">
                <button type="button" data-action="share" class="rounded-lg bg-[#5865f2] px-4 py-2 text-sm font-medium text-white hover:bg-[#4752c4]">{{ __('Compartilhar tela') }}</button>
                <button type="button" data-action="stop-share" class="hidden rounded-lg bg-[#da373c] px-4 py-2 text-sm font-medium text-white hover:bg-[#a12828]">{{ __('Parar') }}</button>

                <select data-quality class="rounded-lg bg-[#1e1f22] px-3 py-2 text-sm text-[#b5bac1]">
                    <option value="720">720p</option>
                    <option value="1080" selected>1080p</option>
                    <option value="1440">1440p</option>
                </select>

                <button type="button" data-action="leave" class="rounded-lg bg-[#4e5058] px-4 py-2 text-sm font-medium text-white hover:bg-[#da373c]">{{ __('Sair da chamada') }}</button>

                <span data-voice-stats class="ml-3 font-mono text-xs text-[#949ba4]"></span>
            </div>
        </section>

        {{-- Chat de texto --}}
        <section data-text-stage x-show="! inCall" class="flex min-h-0 flex-1 flex-col">
            @if ($this->currentChannel)
                <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4" x-data x-init="$el.scrollTop = $el.scrollHeight">
                    @forelse ($this->messages as $message)
                        <article wire:key="msg-{{ $message->id }}" class="flex gap-3">
                            <div class="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-sm font-semibold text-white">
                                @if ($message->user->avatar_url)
                                    <img src="{{ $message->user->avatar_url }}" alt="" class="size-10 object-cover">
                                @else
                                    {{ $message->user->initials() }}
                                @endif
                            </div>

                            <div class="min-w-0">
                                <p class="text-sm">
                                    <span class="font-medium text-white">{{ $message->user->displayName() }}</span>
                                    <span class="ml-2 text-xs text-[#949ba4]">{{ $message->created_at->format('d/m H:i') }}</span>
                                </p>
                                <p class="whitespace-pre-wrap break-words text-[#dbdee1]">{{ $message->content }}</p>
                            </div>
                        </article>
                    @empty
                        <p class="pt-8 text-center text-sm text-[#949ba4]">{{ __('Nenhuma mensagem ainda. Manda a primeira.') }}</p>
                    @endforelse
                </div>

                <form wire:submit="sendMessage" class="shrink-0 px-4 pb-5">
                    <input
                        wire:model="draft"
                        type="text"
                        maxlength="2000"
                        placeholder="{{ __('Conversar em') }} #{{ $this->currentChannel->name }}"
                        class="w-full rounded-lg border-0 bg-[#383a40] px-4 py-3 text-[#dbdee1] placeholder-[#6d6f78] focus:ring-0"
                    >
                </form>
            @else
                <div class="flex flex-1 items-center justify-center px-6 text-center">
                    <div>
                        <p class="text-lg font-medium text-white">{{ __('Nenhum servidor ainda') }}</p>
                        <p class="mt-1 text-sm text-[#949ba4]">{{ __('Crie um servidor no botão + ou entre por um link de convite.') }}</p>
                    </div>
                </div>
            @endif
        </section>
    </main>

    {{-- Membros --}}
    @if ($this->showMembers && $this->currentServer)
        <aside class="w-60 shrink-0 overflow-y-auto bg-[#2b2d31] px-2 py-4">
            <p class="px-2 pb-2 text-xs font-bold uppercase tracking-wide text-[#949ba4]">
                {{ __('Membros') }} — {{ $this->members->count() }}
            </p>

            @foreach ($this->members as $member)
                <div wire:key="member-{{ $member->id }}" class="group flex items-center gap-2 rounded px-2 py-1.5 hover:bg-[#35373c]">
                    <div class="relative flex size-8 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-xs font-semibold text-white">
                        @if ($member->user->avatar_url)
                            <img src="{{ $member->user->avatar_url }}" alt="" class="size-8 object-cover">
                        @else
                            {{ $member->user->initials() }}
                        @endif
                    </div>

                    <span class="min-w-0 flex-1 truncate text-sm {{ $member->role === 'owner' ? 'text-[#f0b232]' : 'text-[#949ba4]' }}">
                        {{ $member->nickname ?? $member->user->displayName() }}
                    </span>

                    @if ($this->viewerMember?->canModerate() && $member->role !== 'owner')
                        <button
                            type="button"
                            wire:click="stopBroadcast('{{ $member->user_id }}')"
                            class="hidden rounded p-1 text-[#b5bac1] hover:text-[#f0b232] group-hover:block"
                            title="{{ __('Encerrar transmissão') }}"
                        >
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M3 3l18 18M21 7v10a1 1 0 01-1 1H9M4 5h1a1 1 0 00-1 1v11a1 1 0 001 1h4"/></svg>
                        </button>

                        <button
                            type="button"
                            wire:click="kickMember('{{ $member->id }}')"
                            wire:confirm="{{ __('Expulsar :nome do servidor?', ['nome' => $member->user->displayName()]) }}"
                            class="hidden rounded p-1 text-[#b5bac1] hover:text-[#da373c] group-hover:block"
                            title="{{ __('Expulsar') }}"
                        >
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0 0 13.5 3h-6a2.25 2.25 0 0 0-2.25 2.25v13.5A2.25 2.25 0 0 0 7.5 21h6a2.25 2.25 0 0 0 2.25-2.25V15M12 9l-3 3m0 0 3 3m-3-3h12.75"/></svg>
                        </button>
                    @endif
                </div>
            @endforeach
        </aside>
    @endif

    {{-- Modal: criar servidor --}}
    @if ($this->creatingServer)
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4" wire:click.self="$set('creatingServer', false)">
            <div class="w-full max-w-sm rounded-lg bg-[#313338] p-6">
                <h2 class="text-xl font-semibold text-white">{{ __('Criar servidor') }}</h2>
                <p class="mt-1 text-sm text-[#b5bac1]">{{ __('Ele já nasce com um canal de texto e um de voz.') }}</p>

                <form wire:submit="createServer" class="mt-5">
                    <label class="text-xs font-bold uppercase tracking-wide text-[#b5bac1]">{{ __('Nome do servidor') }}</label>
                    <input wire:model="newServerName" type="text" autofocus class="mt-2 w-full rounded border-0 bg-[#1e1f22] px-3 py-2.5 text-white focus:ring-0">
                    @error('newServerName') <p class="mt-1 text-sm text-[#f23f43]">{{ $message }}</p> @enderror

                    <div class="mt-6 flex justify-end gap-3">
                        <button type="button" wire:click="$set('creatingServer', false)" class="px-4 py-2 text-sm text-white hover:underline">{{ __('Cancelar') }}</button>
                        <button type="submit" class="rounded bg-[#5865f2] px-5 py-2 text-sm font-medium text-white hover:bg-[#4752c4]">{{ __('Criar') }}</button>
                    </div>
                </form>
            </div>
        </div>
    @endif

    {{-- Modal: convite --}}
    @if ($this->currentServer)
        <div x-cloak x-show="invite" class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4" x-on:click.self="invite = false">
            <div class="w-full max-w-sm rounded-lg bg-[#313338] p-6">
                <h2 class="text-xl font-semibold text-white">{{ __('Convidar para :servidor', ['servidor' => $this->currentServer->name]) }}</h2>
                <p class="mt-1 text-sm text-[#b5bac1]">{{ __('Só entra quem estiver logado.') }}</p>

                <div class="mt-5 flex gap-2" x-data="{ copied: false }">
                    <input readonly value="{{ $this->inviteUrl }}" class="w-full rounded border-0 bg-[#1e1f22] px-3 py-2.5 text-sm text-[#dbdee1] focus:ring-0">
                    <button
                        type="button"
                        x-on:click="navigator.clipboard.writeText('{{ $this->inviteUrl }}'); copied = true; setTimeout(() => copied = false, 2000)"
                        class="shrink-0 rounded bg-[#5865f2] px-4 text-sm font-medium text-white hover:bg-[#4752c4]"
                        x-text="copied ? '{{ __('Copiado') }}' : '{{ __('Copiar') }}'"
                    ></button>
                </div>
            </div>
        </div>
    @endif
</div>
