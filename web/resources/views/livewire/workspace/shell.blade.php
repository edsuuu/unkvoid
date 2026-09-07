<div class="flex h-screen w-screen gap-2 overflow-hidden bg-[#151619] p-2 text-[#dbdee1]"
     data-me="{{ auth()->user()->displayName() }}"
     @if ($this->currentServer) data-server-id="{{ $this->currentServer->id }}" @endif
     data-me-avatar="{{ auth()->user()->avatar_url }}" x-data="{ invite: false, userMenu: false, inCall: false, connecting: false, viewing: 'text', members: (localStorage.getItem('ui:members') ?? '1') === '1', channels: (localStorage.getItem('ui:channels') ?? '1') === '1', voiceChannel: '', voiceChannelId: '', voiceClock: '', voiceStatus: '{{ __('Disponível') }}' }"
     x-on:voice-state.window="inCall = $event.detail.inCall; connecting = false; viewing = $event.detail.inCall ? 'voice' : 'text'; voiceChannel = $event.detail.channelName ?? voiceChannel; voiceChannelId = $event.detail.channelId ?? ''"
     x-on:voice-connecting.window="connecting = true; viewing = 'voice'"
     x-on:stage-changed.window="viewing = $event.detail.stage"
     x-on:voice-status.window="voiceStatus = $event.detail.text"
     x-on:voice-clock.window="voiceClock = $event.detail.label">

    {{-- Trilha de servidores --}}
    <nav class="flex w-[68px] shrink-0 flex-col items-center gap-2 overflow-y-auto rounded-lg bg-[#1e1f22] py-3">
        <button
            type="button"
            wire:click="goHome"
            class="group relative flex size-12 cursor-pointer items-center justify-center"
            title="{{ __('Início') }}"
        >
            <span @class([
                'absolute -left-3 w-1 rounded-r-full bg-white transition-all duration-200',
                'h-10' => ! $this->currentServer,
                'h-0 group-hover:h-5' => (bool) $this->currentServer,
            ])></span>

            <span @class([
                'flex size-12 items-center justify-center transition-all duration-200',
                'rounded-2xl bg-[#5865f2] text-white' => ! $this->currentServer,
                'rounded-[24px] bg-[#313338] text-[#dbdee1] group-hover:rounded-2xl group-hover:bg-[#5865f2] group-hover:text-white' => (bool) $this->currentServer,
            ])>
                <svg class="size-6" fill="currentColor" viewBox="0 0 24 24"><path d="M11.47 3.84a.75.75 0 0 1 1.06 0l8.69 8.69a.75.75 0 1 0 1.06-1.06l-8.689-8.69a2.25 2.25 0 0 0-3.182 0l-8.69 8.69a.75.75 0 1 0 1.061 1.06l8.69-8.69z"/><path d="m12 5.432 8.159 8.159c.03.03.06.058.091.086v6.198c0 1.035-.84 1.875-1.875 1.875H15a.75.75 0 0 1-.75-.75v-4.5a.75.75 0 0 0-.75-.75h-3a.75.75 0 0 0-.75.75V21a.75.75 0 0 1-.75.75H5.625a1.875 1.875 0 0 1-1.875-1.875v-6.198a2.29 2.29 0 0 0 .091-.086L12 5.432z"/></svg>
            </span>
        </button>

        <span class="h-0.5 w-8 shrink-0 rounded-full bg-[#35373c]"></span>

        @foreach ($this->servers as $server)
            <button
                type="button"
                wire:key="server-{{ $server->id }}"
                wire:click="selectServer('{{ $server->id }}')"
                class="group relative flex size-12 cursor-pointer items-center justify-center"
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
            class="flex size-12 cursor-pointer items-center justify-center rounded-[24px] bg-[#313338] text-[#23a55a] transition-all duration-200 hover:rounded-2xl hover:bg-[#23a55a] hover:text-white"
            title="{{ __('Criar servidor') }}"
        >
            <svg class="size-6" fill="none" stroke="currentColor" stroke-width="2.2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M12 5v14M5 12h14"/></svg>
        </button>
    </nav>

    {{-- Sidebar de canais --}}
    <aside
        x-show="channels"
        x-cloak
        x-transition:enter="transition-all duration-200 ease-out"
        x-transition:enter-start="w-0 opacity-0"
        x-transition:enter-end="w-60 opacity-100"
        x-transition:leave="transition-all duration-150 ease-in"
        x-transition:leave-start="w-60 opacity-100"
        x-transition:leave-end="w-0 opacity-0"
        class="z-10 flex w-60 shrink-0 flex-col overflow-hidden rounded-lg bg-[#2b2d31]"
    >
        <header class="flex h-12 shrink-0 items-center justify-between border-b border-[#26282c] px-4">
            <span class="truncate font-semibold text-white">{{ $this->currentServer?->name ?? __('Início') }}</span>

            @if ($this->currentServer)
                <div class="relative" x-data="{ menu: false }" x-on:click.outside="menu = false">
                    <button type="button" x-on:click="menu = ! menu" class="cursor-pointer rounded p-1 text-[#b5bac1] transition-colors hover:bg-[#35373c] hover:text-white" title="{{ __('Menu do servidor') }}">
                        <svg class="size-[18px]" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M4 6h16M4 12h16M4 18h16"/></svg>
                    </button>

                    <div x-show="menu" x-cloak x-transition.opacity.duration.120ms
                        class="absolute right-0 top-9 z-40 w-52 overflow-hidden rounded-lg bg-[#111214] py-1.5 shadow-2xl">
                        <button type="button" x-on:click="menu = false; invite = true"
                            class="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left text-sm text-[#dbdee1] hover:bg-[#5865f2] hover:text-white">
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M13.19 8.688a4.5 4.5 0 0 1 1.242 7.244l-4.5 4.5a4.5 4.5 0 0 1-6.364-6.364l1.757-1.757m13.35-.622 1.757-1.757a4.5 4.5 0 0 0-6.364-6.364l-4.5 4.5a4.5 4.5 0 0 0 1.242 7.244"/></svg>
                            {{ __('Convidar pessoas') }}
                        </button>

                        <button type="button" x-on:click="menu = false" wire:click="openServerSettings"
                            class="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left text-sm text-[#dbdee1] hover:bg-[#5865f2] hover:text-white">
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="1.8" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M10.34 3.94c.09-.54.55-.94 1.1-.94h1.12c.55 0 1.01.4 1.1.94l.15.9c.44.16.86.38 1.24.65l.85-.32c.51-.2 1.09 0 1.37.48l.56.97c.28.48.15 1.09-.29 1.42l-.72.54c.04.24.06.48.06.72s-.02.48-.06.72l.72.54c.44.33.57.94.29 1.42l-.56.97c-.28.48-.86.68-1.37.48l-.85-.32c-.38.27-.8.49-1.24.65l-.15.9c-.09.54-.55.94-1.1.94h-1.12c-.55 0-1.01-.4-1.1-.94l-.15-.9a5.7 5.7 0 0 1-1.24-.65l-.85.32c-.51.2-1.09 0-1.37-.48l-.56-.97a1.12 1.12 0 0 1 .29-1.42l.72-.54a5.6 5.6 0 0 1 0-1.44l-.72-.54a1.12 1.12 0 0 1-.29-1.42l.56-.97c.28-.48.86-.68 1.37-.48l.85.32c.38-.27.8-.49 1.24-.65l.15-.9z"/><circle cx="12" cy="12" r="2.5"/></svg>
                            {{ __('Configurações') }}
                        </button>

                        <button type="button" x-on:click="menu = false" wire:click="startChannel('text')"
                            class="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left text-sm text-[#dbdee1] hover:bg-[#5865f2] hover:text-white">
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M12 5v14M5 12h14"/></svg>
                            {{ __('Criar canal') }}
                        </button>
                    </div>
                </div>
            @endif
        </header>

        <div class="flex-1 overflow-y-auto px-2 py-3">
            @unless ($this->currentServer)
                <p class="px-2 pb-1 pt-2 text-xs font-bold uppercase tracking-wide text-[#949ba4]">{{ __('Seus servidores') }}</p>

                @forelse ($this->servers as $server)
                    <button
                        type="button"
                        wire:key="home-{{ $server->id }}"
                        wire:click="selectServer('{{ $server->id }}')"
                        class="flex w-full cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-left text-[15px] text-[#949ba4] hover:bg-[#35373c] hover:text-[#dbdee1]"
                    >
                        <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-[#5865f2] text-[10px] font-semibold text-white">{{ $server->initials() }}</span>
                        <span class="truncate">{{ $server->name }}</span>
                    </button>
                @empty
                    <p class="px-2 py-3 text-sm text-[#6d6f78]">{{ __('Nenhum ainda.') }}</p>
                @endforelse
            @endunless

            @if ($this->currentServer)
                <div class="group/section flex items-center justify-between px-2 pb-1 pt-2">
                    <p class="text-xs font-bold uppercase tracking-wide text-[#949ba4]">{{ __('Canais de texto') }}</p>
                    @if ($this->viewerMember?->canModerate())
                        <button type="button" wire:click="startChannel('text')" title="{{ __('Criar canal de texto') }}"
                            class="cursor-pointer text-[#949ba4] opacity-0 transition hover:text-white group-hover/section:opacity-100"><svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M12 5v14M5 12h14"/></svg></button>
                    @endif
                </div>

                @foreach ($this->channels->where('type', 'text') as $channel)
                    <div wire:key="channel-{{ $channel->id }}" class="group/row flex items-center">
                    <button
                        type="button"
                        wire:click="selectChannel('{{ $channel->id }}')"
                        @class([
                            'group flex w-full cursor-pointer items-center gap-1.5 rounded px-2 py-1.5 text-left text-[15px] transition-colors duration-100',
                            'bg-[#404249] text-white' => $channel->id === $this->channelId,
                            'text-[#949ba4] hover:bg-[#35373c] hover:text-[#dbdee1]' => $channel->id !== $this->channelId,
                        ])
                    >
                        <span class="text-xl leading-none text-[#80848e]">#</span>
                        <span class="truncate">{{ $channel->name }}</span>
                    </button>

                    @if ($this->viewerMember?->canModerate())
                        <button type="button" wire:click="startRenameChannel('{{ $channel->id }}')" title="{{ __('Renomear') }}"
                            class="cursor-pointer rounded p-1 text-[#949ba4] opacity-0 transition hover:text-white group-hover/row:opacity-100"><svg class="size-3.5" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="m16.86 4.49 2.65 2.65m-1.4-3.9a1.87 1.87 0 1 1 2.65 2.65L7.5 18.15l-3.5.85.85-3.5L18.11 3.24z"/></svg></button>
                    @endif
                    </div>
                @endforeach
            @endif

            @if ($this->currentServer)
                <div class="group/section flex items-center justify-between px-2 pb-1 pt-4">
                    <p class="text-xs font-bold uppercase tracking-wide text-[#949ba4]">{{ __('Canais de voz') }}</p>
                    @if ($this->viewerMember?->canModerate())
                        <button type="button" wire:click="startChannel('voice')" title="{{ __('Criar canal de voz') }}"
                            class="cursor-pointer text-[#949ba4] opacity-0 transition hover:text-white group-hover/section:opacity-100"><svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M12 5v14M5 12h14"/></svg></button>
                    @endif
                </div>

                @foreach ($this->channels->where('type', 'voice') as $channel)
                    <div wire:key="voice-{{ $channel->id }}" x-data>
                        <div class="group/row flex items-center">
                        <button
                            type="button"
                            wire:click="selectChannel('{{ $channel->id }}')"
                            x-on:click="if (voiceChannelId !== '{{ $channel->id }}') { connecting = true; } viewing = 'voice'"
                            class="group flex w-full cursor-pointer items-center gap-1.5 rounded px-2 py-1.5 text-left text-[15px] text-[#949ba4] transition-colors duration-100 hover:bg-[#35373c] hover:text-[#dbdee1]"
                        >
                            <svg class="size-5 shrink-0 text-[#80848e]" fill="currentColor" viewBox="0 0 24 24"><path d="M11.383 3.076A1 1 0 0 1 12 4v16a1 1 0 0 1-1.707.707L5.586 16H3a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1h2.586l4.707-4.707a1 1 0 0 1 1.09-.217zM16.5 7.5a1 1 0 0 1 1.414 0 6 6 0 0 1 0 8.486 1 1 0 1 1-1.414-1.414 4 4 0 0 0 0-5.658 1 1 0 0 1 0-1.414z"/></svg>
                            <span class="truncate">{{ $channel->name }}</span>

                            <span
                                wire:ignore
                                data-channel-clock="{{ $channel->id }}"
                                class="ml-auto shrink-0 font-mono text-xs text-[#23a55a]"
                            ></span>
                        </button>

                        @if ($this->viewerMember?->canModerate())
                            <button type="button" wire:click="startRenameChannel('{{ $channel->id }}')" title="{{ __('Renomear') }}"
                                class="cursor-pointer rounded p-1 text-[#949ba4] opacity-0 transition hover:text-white group-hover/row:opacity-100"><svg class="size-3.5" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="m16.86 4.49 2.65 2.65m-1.4-3.9a1.87 1.87 0 1 1 2.65 2.65L7.5 18.15l-3.5.85.85-3.5L18.11 3.24z"/></svg></button>
                        @endif
                        </div>

                        <div wire:ignore class="ml-6 space-y-0.5" data-voice-members="{{ $channel->id }}"></div>
                    </div>
                @endforeach
            @endif
        </div>

        {{-- Faixa de voz conectada. wire:ignore: o Livewire não pode recriar isto,
             é o único ponto de saída da chamada. --}}
        <div wire:ignore x-show="inCall" x-cloak class="shrink-0 border-t border-[#26282c] bg-[#232428] px-2 py-2">
            <div class="flex items-center gap-2 px-2">
                <span class="size-2 shrink-0 rounded-full bg-[#23a55a]"></span>
                <div class="min-w-0 flex-1 leading-tight">
                    <p class="truncate text-sm font-medium text-[#23a55a]">
                        {{ __('Voz conectada') }}
                        <span class="font-mono text-xs text-[#949ba4]" x-text="voiceClock"></span>
                    </p>
                    <p class="truncate text-xs text-[#949ba4]" x-text="voiceChannel"></p>
                </div>

                <button
                    type="button"
                    data-action="leave"
                    title="{{ __('Desconectar') }}"
                    class="cursor-pointer rounded p-1.5 text-[#b5bac1] hover:bg-[#35373c] hover:text-[#f23f43]"
                >
                    <svg class="size-5" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0 0 13.5 3h-6a2.25 2.25 0 0 0-2.25 2.25v13.5A2.25 2.25 0 0 0 7.5 21h6a2.25 2.25 0 0 0 2.25-2.25V15M12 9l-3 3m0 0 3 3m-3-3h12.75"/></svg>
                </button>
            </div>

            <div class="mt-2 flex gap-2">
                <select
                    data-quality
                    title="{{ __('Qualidade da transmissão') }}"
                    class="w-[68px] shrink-0 cursor-pointer rounded border-0 bg-[#1e1f22] px-1.5 py-1.5 text-xs text-[#b5bac1] focus:ring-0"
                >
                    <option value="720">720p</option>
                    <option value="1080" selected>1080p</option>
                    <option value="1440">1440p</option>
                </select>

                <button
                    type="button"
                    data-action="share"
                    class="flex-1 cursor-pointer whitespace-nowrap rounded bg-[#5865f2] px-2 py-1.5 text-sm font-medium text-white hover:bg-[#4752c4]"
                >{{ __('Compartilhar') }}</button>
            </div>

            <button
                type="button"
                data-action="stop-share"
                class="mt-2 hidden w-full cursor-pointer rounded bg-[#da373c] px-3 py-1.5 text-sm font-medium text-white hover:bg-[#a12828]"
            >{{ __('Parar de compartilhar') }}</button>
        </div>

        {{-- Painel do usuário --}}
        <div class="relative flex h-[52px] shrink-0 items-center gap-2 bg-[#232428] px-2">
            <div x-show="userMenu" x-cloak x-on:click.outside="userMenu = false"
                class="absolute bottom-[56px] left-2 z-40 w-56 overflow-hidden rounded-lg bg-[#111214] py-2 shadow-2xl">
                <div class="border-b border-[#2b2d31] px-3 pb-2">
                    <p class="truncate text-sm font-medium text-white">{{ auth()->user()->displayName() }}</p>
                    <p class="truncate text-xs text-[#949ba4]">{{ auth()->user()->email }}</p>
                </div>

                <a href="{{ route('profile.edit') }}" wire:navigate class="block cursor-pointer px-3 py-2 text-sm text-[#dbdee1] hover:bg-[#5865f2] hover:text-white">
                    {{ __('Minha conta') }}
                </a>

                <button type="button" x-on:click="$dispatch('logout')"
                    class="w-full cursor-pointer px-3 py-2 text-left text-sm text-[#f23f43] hover:bg-[#da373c] hover:text-white">
                    {{ __('Sair da conta') }}
                </button>
            </div>

            <button type="button" x-on:click="userMenu = ! userMenu"
                class="flex min-w-0 flex-1 cursor-pointer items-center gap-2 rounded px-1 py-1 text-left hover:bg-[#35373c]">
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
            </button>

            <button type="button" data-action="toggle-mic" class="cursor-pointer rounded p-1.5 text-[#b5bac1] transition-colors hover:bg-[#35373c] hover:text-white" title="{{ __('Microfone') }}">
                <svg class="size-5" fill="currentColor" viewBox="0 0 24 24"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 1 0-6 0v5a3 3 0 0 0 3 3z"/><path d="M18 11a1 1 0 1 0-2 0 4 4 0 0 1-8 0 1 1 0 1 0-2 0 6 6 0 0 0 5 5.917V19H9a1 1 0 1 0 0 2h6a1 1 0 1 0 0-2h-2v-2.083A6 6 0 0 0 18 11z"/></svg>
            </button>

            <a href="{{ route('profile.edit') }}" wire:navigate class="cursor-pointer rounded p-1.5 text-[#b5bac1] transition-colors hover:bg-[#35373c] hover:text-white" title="{{ __('Configurações') }}">
                <svg class="size-5" fill="none" stroke="currentColor" stroke-width="1.8" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M10.34 3.94c.09-.54.55-.94 1.1-.94h1.12c.55 0 1.01.4 1.1.94l.15.9c.44.16.86.38 1.24.65l.85-.32c.51-.2 1.09 0 1.37.48l.56.97c.28.48.15 1.09-.29 1.42l-.72.54c.04.24.06.48.06.72s-.02.48-.06.72l.72.54c.44.33.57.94.29 1.42l-.56.97c-.28.48-.86.68-1.37.48l-.85-.32c-.38.27-.8.49-1.24.65l-.15.9c-.09.54-.55.94-1.1.94h-1.12c-.55 0-1.01-.4-1.1-.94l-.15-.9a5.7 5.7 0 0 1-1.24-.65l-.85.32c-.51.2-1.09 0-1.37-.48l-.56-.97a1.12 1.12 0 0 1 .29-1.42l.72-.54a5.6 5.6 0 0 1 0-1.44l-.72-.54a1.12 1.12 0 0 1-.29-1.42l.56-.97c.28-.48.86-.68 1.37-.48l.85.32c.38-.27.8-.49 1.24-.65l.15-.9z"/><circle cx="12" cy="12" r="2.5"/></svg>
            </a>
        </div>
    </aside>

    {{-- Conteúdo --}}
    <main class="flex min-w-0 flex-1 flex-col overflow-hidden rounded-lg bg-[#313338]">
        <header class="flex h-12 shrink-0 items-center gap-2 border-b border-[#3a3c41] px-4">
            <button
                type="button"
                x-on:click="channels = ! channels; localStorage.setItem('ui:channels', channels ? '1' : '0')"
                x-bind:title="channels ? '{{ __('Ocultar canais') }}' : '{{ __('Mostrar canais') }}'"
                class="-ml-1 cursor-pointer rounded p-1.5 text-[#b5bac1] transition-colors hover:bg-[#35373c] hover:text-white"
            >
                <svg class="size-[18px]" fill="none" stroke="currentColor" stroke-width="1.8" viewBox="0 0 24 24">
                    <rect x="3" y="4" width="18" height="16" rx="2"/>
                    <path d="M9 4v16" x-bind:class="channels ? '' : 'opacity-40'"/>
                </svg>
            </button>

            @if ($this->currentChannel)
                <span class="text-xl text-[#80848e]">#</span>
                <span class="font-semibold text-white">{{ $this->currentChannel->name }}</span>
            @else
                <span class="font-semibold text-white">{{ __('Selecione um canal') }}</span>
            @endif

            <span class="flex-1"></span>

            <button
                type="button"
                x-on:click="members = ! members; localStorage.setItem('ui:members', members ? '1' : '0')"
                x-bind:class="members ? 'text-white' : 'text-[#b5bac1]'"
                class="cursor-pointer rounded p-1.5 transition-colors hover:bg-[#35373c]"
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
            x-show="viewing === 'voice'"
            x-cloak
            class="flex min-h-0 flex-1 flex-col bg-[#1e1f22]"
        >
            <div class="relative min-h-0 flex-1">
                <div data-voice-grid class="grid h-full min-h-0 gap-3 overflow-y-auto p-4"></div>

                <div x-show="connecting" x-cloak class="absolute inset-0 flex items-center justify-center gap-3 bg-[#1e1f22] text-[#b5bac1]">
                    <span class="size-4 animate-spin rounded-full border-2 border-[#4e5058] border-t-[#5865f2]"></span>
                    {{ __('Conectando…') }}
                </div>

                <p x-show="! connecting" x-cloak data-voice-empty class="pointer-events-none absolute inset-0 flex items-center justify-center text-sm text-[#949ba4]">
                    {{ __('Ninguém compartilhando tela ainda') }}
                </p>
            </div>

            <div class="flex shrink-0 items-center justify-center gap-3 border-t border-[#26282c] bg-[#232428] px-4 py-3">
                <button
                    type="button"
                    data-action="fullscreen-grid"
                    title="{{ __('Ver as transmissões em tela cheia (até 4)') }}"
                    class="flex cursor-pointer items-center gap-2 rounded-lg bg-[#4e5058] px-3 py-1.5 text-sm font-medium text-white transition hover:bg-[#5865f2]"
                >
                    <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M4 9V5a1 1 0 0 1 1-1h4M20 9V5a1 1 0 0 0-1-1h-4M4 15v4a1 1 0 0 0 1 1h4M20 15v4a1 1 0 0 1-1 1h-4"/></svg>
                    {{ __('Tela cheia') }}
                </button>

                <span data-voice-stats class="ml-3 font-mono text-xs text-[#949ba4]"></span>
            </div>
        </section>

        {{-- Chat de texto --}}
        <section data-text-stage x-show="viewing === 'text'" class="flex min-h-0 flex-1 flex-col">
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
                <div class="flex-1 overflow-y-auto px-8 py-10">
                    <div class="mx-auto max-w-3xl">
                        <h1 class="text-2xl font-semibold text-white">
                            {{ __('Olá, :nome', ['nome' => auth()->user()->displayName()]) }}
                        </h1>
                        <p class="mt-1 text-sm text-[#949ba4]">
                            {{ __('Escolha um servidor para conversar ou entrar em um canal de voz.') }}
                        </p>

                        <div class="mt-8 grid gap-3 sm:grid-cols-2">
                            @foreach ($this->servers as $server)
                                <button
                                    type="button"
                                    wire:key="card-{{ $server->id }}"
                                    wire:click="selectServer('{{ $server->id }}')"
                                    class="group flex cursor-pointer items-center gap-3 rounded-lg bg-[#2b2d31] p-4 text-left transition hover:bg-[#35373c]"
                                >
                                    <span class="flex size-12 shrink-0 items-center justify-center rounded-2xl bg-[#5865f2] font-semibold text-white">{{ $server->initials() }}</span>
                                    <span class="min-w-0">
                                        <span class="block truncate font-medium text-white">{{ $server->name }}</span>
                                        <span class="block text-xs text-[#949ba4]">
                                            {{ trans_choice('{1} :count membro|[2,*] :count membros', $server->members()->count(), ['count' => $server->members()->count()]) }}
                                        </span>
                                    </span>
                                </button>
                            @endforeach

                            <button
                                type="button"
                                wire:click="$set('creatingServer', true)"
                                class="group flex cursor-pointer items-center gap-3 rounded-lg border border-dashed border-[#3f4147] p-4 text-left text-[#949ba4] transition hover:border-[#23a55a] hover:text-white"
                            >
                                <span class="flex size-12 shrink-0 items-center justify-center rounded-2xl bg-[#313338] text-[#23a55a] transition group-hover:bg-[#23a55a] group-hover:text-white">
                                        <svg class="size-6" fill="none" stroke="currentColor" stroke-width="2.2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M12 5v14M5 12h14"/></svg>
                                    </span>
                                <span class="min-w-0">
                                    <span class="block font-medium">{{ __('Criar servidor') }}</span>
                                    <span class="block text-xs">{{ __('Nasce com canal de texto e de voz') }}</span>
                                </span>
                            </button>
                        </div>

                        <p class="mt-8 text-sm text-[#6d6f78]">
                            {{ __('Para entrar em um servidor de outra pessoa, use o link de convite que ela te mandar.') }}
                        </p>
                    </div>
                </div>
            @endif
        </section>
    </main>

    {{-- Membros --}}
    @if ($this->currentServer)
        <aside
            x-show="members"
            x-cloak
            x-transition:enter="transition-all duration-200 ease-out"
            x-transition:enter-start="w-0 opacity-0"
            x-transition:enter-end="w-60 opacity-100"
            x-transition:leave="transition-all duration-150 ease-in"
            x-transition:leave-start="w-60 opacity-100"
            x-transition:leave-end="w-0 opacity-0"
            class="w-60 shrink-0 overflow-y-auto rounded-lg bg-[#2b2d31] px-2 py-4"
        >
            <button
                type="button"
                wire:click="$set('showAllMembers', true)"
                class="mb-2 flex w-full cursor-pointer items-center justify-between rounded px-2 py-1 text-xs font-bold uppercase tracking-wide text-[#949ba4] hover:bg-[#35373c] hover:text-[#dbdee1]"
            >
                <span>{{ __('Membros') }} — {{ $this->members->count() }}</span>
                <span class="font-normal normal-case tracking-normal">{{ __('ver todos') }}</span>
            </button>

            @foreach ($this->members as $member)
                <div wire:key="member-{{ $member->id }}" class="group flex items-center gap-2 rounded px-2 py-1.5 transition-colors duration-100 hover:bg-[#35373c]">
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
                            class="hidden cursor-pointer rounded p-1 text-[#b5bac1] hover:text-[#f0b232] group-hover:block"
                            title="{{ __('Encerrar a transmissão (continua na sala)') }}"
                        >
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M3 3l18 18M21 7v10a1 1 0 01-1 1H9M4 5h1a1 1 0 00-1 1v11a1 1 0 001 1h4"/></svg>
                        </button>

                        <button
                            type="button"
                            wire:click="disconnectFromVoice('{{ $member->user_id }}')"
                            class="hidden cursor-pointer rounded p-1 text-[#b5bac1] hover:text-[#f0b232] group-hover:block"
                            title="{{ __('Tirar da chamada (continua no chat)') }}"
                        >
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0 0 13.5 3h-6a2.25 2.25 0 0 0-2.25 2.25v13.5A2.25 2.25 0 0 0 7.5 21h6a2.25 2.25 0 0 0 2.25-2.25V15M12 9l-3 3m0 0 3 3m-3-3h12.75"/></svg>
                        </button>

                        <button
                            type="button"
                            wire:click="removeMember('{{ $member->id }}')"
                            wire:confirm="{{ __('Remover :nome do servidor? Ela perde o acesso ao chat também.', ['nome' => $member->user->displayName()]) }}"
                            class="hidden cursor-pointer rounded p-1 text-[#b5bac1] hover:text-[#da373c] group-hover:block"
                            title="{{ __('Remover do servidor') }}"
                        >
                            <svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18 18 6M6 6l12 12"/></svg>
                        </button>
                    @endif
                </div>
            @endforeach
        </aside>
    @endif

    {{-- Modal: criar servidor --}}
    @if ($this->creatingServer)
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-[2px]" wire:click.self="$set('creatingServer', false)">
            <div class="w-full max-w-sm rounded-lg bg-[#313338] p-6">
                <h2 class="text-xl font-semibold text-white">{{ __('Criar servidor') }}</h2>
                <p class="mt-1 text-sm text-[#b5bac1]">{{ __('Ele já nasce com um canal de texto e um de voz.') }}</p>

                <form wire:submit="createServer" class="mt-5">
                    <label class="text-xs font-bold uppercase tracking-wide text-[#b5bac1]">{{ __('Nome do servidor') }}</label>
                    <input wire:model="newServerName" type="text" autofocus class="mt-2 w-full rounded border-0 bg-[#1e1f22] px-3 py-2.5 text-white focus:ring-0">
                    @error('newServerName') <p class="mt-1 text-sm text-[#f23f43]">{{ $message }}</p> @enderror

                    <div class="mt-6 flex justify-end gap-3">
                        <button type="button" wire:click="$set('creatingServer', false)" class="cursor-pointer px-4 py-2 text-sm text-white hover:underline">{{ __('Cancelar') }}</button>
                        <button type="submit" class="cursor-pointer rounded bg-[#5865f2] px-5 py-2 text-sm font-medium text-white hover:bg-[#4752c4]">{{ __('Criar') }}</button>
                    </div>
                </form>
            </div>
        </div>
    @endif

    <form method="POST" action="{{ route('logout') }}" x-on:logout.window="$el.submit()" class="hidden">
        @csrf
    </form>

    {{-- Modal: renomear canal --}}
    @if ($this->renamingChannelId)
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-[2px]" wire:click.self="$set('renamingChannelId', null)">
            <div class="w-full max-w-sm rounded-lg bg-[#313338] p-6">
                <h2 class="text-xl font-semibold text-white">{{ __('Renomear canal') }}</h2>

                <form wire:submit="renameChannel" class="mt-5">
                    <input wire:model="renamedChannel" type="text" autofocus class="w-full rounded border-0 bg-[#1e1f22] px-3 py-2.5 text-white focus:ring-0">
                    @error('renamedChannel') <p class="mt-1 text-sm text-[#f23f43]">{{ $message }}</p> @enderror

                    <div class="mt-6 flex justify-end gap-3">
                        <button type="button" wire:click="$set('renamingChannelId', null)" class="cursor-pointer px-4 py-2 text-sm text-white hover:underline">{{ __('Cancelar') }}</button>
                        <button type="submit" class="cursor-pointer rounded bg-[#5865f2] px-5 py-2 text-sm font-medium text-white hover:bg-[#4752c4]">{{ __('Salvar') }}</button>
                    </div>
                </form>
            </div>
        </div>
    @endif

    {{-- Modal: criar canal --}}
    @if ($this->creatingChannelType)
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-[2px]" wire:click.self="$set('creatingChannelType', null)">
            <div class="w-full max-w-sm rounded-lg bg-[#313338] p-6">
                <h2 class="text-xl font-semibold text-white">
                    {{ $this->creatingChannelType === 'text' ? __('Novo canal de texto') : __('Novo canal de voz') }}
                </h2>

                <form wire:submit="createChannel" class="mt-5">
                    <label class="text-xs font-bold uppercase tracking-wide text-[#b5bac1]">{{ __('Nome do canal') }}</label>
                    <div class="mt-2 flex items-center rounded bg-[#1e1f22] px-3">
                        <span class="text-[#6d6f78]">
                            @if ($this->creatingChannelType === 'text')
                                #
                            @else
                                <svg class="size-4" fill="currentColor" viewBox="0 0 24 24"><path d="M11.38 3.08A1 1 0 0 1 12 4v16a1 1 0 0 1-1.71.71L5.59 16H3a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1h2.59l4.7-4.71a1 1 0 0 1 1.09-.21zM16.5 7.5a1 1 0 0 1 1.41 0 6 6 0 0 1 0 8.49 1 1 0 1 1-1.41-1.42 4 4 0 0 0 0-5.65 1 1 0 0 1 0-1.42z"/></svg>
                            @endif
                        </span>
                        <input wire:model="newChannelName" type="text" autofocus class="w-full border-0 bg-transparent px-2 py-2.5 text-white focus:ring-0">
                    </div>
                    @error('newChannelName') <p class="mt-1 text-sm text-[#f23f43]">{{ $message }}</p> @enderror

                    <div class="mt-6 flex justify-end gap-3">
                        <button type="button" wire:click="$set('creatingChannelType', null)" class="cursor-pointer px-4 py-2 text-sm text-white hover:underline">{{ __('Cancelar') }}</button>
                        <button type="submit" class="cursor-pointer rounded bg-[#5865f2] px-5 py-2 text-sm font-medium text-white hover:bg-[#4752c4]">{{ __('Criar') }}</button>
                    </div>
                </form>
            </div>
        </div>
    @endif

    {{-- Modal: todos os membros --}}
    @if ($this->showAllMembers && $this->currentServer)
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-[2px]" wire:click.self="$set('showAllMembers', false)">
            <div class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-lg bg-[#313338]">
                <header class="flex shrink-0 items-center justify-between border-b border-[#3f4147] px-6 py-4">
                    <h2 class="text-lg font-semibold text-white">
                        {{ __('Membros de :servidor', ['servidor' => $this->currentServer->name]) }}
                        <span class="ml-1 text-sm font-normal text-[#949ba4]">{{ $this->members->count() }}</span>
                    </h2>
                    <button type="button" wire:click="$set('showAllMembers', false)" class="cursor-pointer rounded p-1 text-[#b5bac1] transition-colors hover:bg-[#3f4147] hover:text-white">
                        <svg class="size-5" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M6 6l12 12M18 6L6 18"/></svg>
                    </button>
                </header>

                <div class="shrink-0 px-6 pt-4">
                    <input
                        wire:model.live.debounce.200ms="memberSearch"
                        type="search"
                        placeholder="{{ __('Buscar membro…') }}"
                        class="w-full rounded border-0 bg-[#1e1f22] px-3 py-2 text-sm text-white placeholder-[#6d6f78] focus:ring-0"
                    >
                </div>

                <div class="min-h-0 flex-1 space-y-1 overflow-y-auto px-4 py-4">
                    @forelse ($this->searchedMembers as $member)
                        <div wire:key="all-{{ $member->id }}" class="group flex items-center gap-3 rounded px-2 py-2 hover:bg-[#2b2d31]">
                            <div class="flex size-9 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-xs font-semibold text-white">
                                @if ($member->user->avatar_url)
                                    <img src="{{ $member->user->avatar_url }}" alt="" class="size-9 object-cover">
                                @else
                                    {{ $member->user->initials() }}
                                @endif
                            </div>

                            <div class="min-w-0 flex-1">
                                <p class="truncate text-sm font-medium text-white">{{ $member->nickname ?? $member->user->displayName() }}</p>
                                <p class="truncate text-xs text-[#949ba4]">
                                    {{ __('entrou em :data', ['data' => $member->joined_at?->format('d/m/Y') ?? '—']) }}
                                </p>
                            </div>

                            <span @class([
                                'shrink-0 rounded px-2 py-0.5 text-xs font-medium',
                                'bg-[#f0b232]/15 text-[#f0b232]' => $member->role === 'owner',
                                'bg-[#5865f2]/15 text-[#949ba4]' => $member->role !== 'owner',
                            ])>{{ $member->role }}</span>

                            @if ($this->viewerMember?->canModerate() && $member->role !== 'owner')
                                <button
                                    type="button"
                                    wire:click="removeMember('{{ $member->id }}')"
                                    wire:confirm="{{ __('Remover :nome do servidor?', ['nome' => $member->user->displayName()]) }}"
                                    class="hidden shrink-0 cursor-pointer rounded p-1 text-[#b5bac1] hover:text-[#da373c] group-hover:block"
                                    title="{{ __('Remover do servidor') }}"
                                ><svg class="size-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" d="M6 6l12 12M18 6L6 18"/></svg></button>
                            @endif
                        </div>
                    @empty
                        <p class="py-6 text-center text-sm text-[#6d6f78]">{{ __('Ninguém com esse nome.') }}</p>
                    @endforelse
                </div>
            </div>
        </div>
    @endif

    {{-- Modal: configurações do servidor --}}
    @if ($this->editingServer && $this->currentServer)
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-[2px]" wire:click.self="$set('editingServer', false)">
            <div class="w-full max-w-md rounded-lg bg-[#313338] p-6">
                <h2 class="text-xl font-semibold text-white">{{ __('Configurações do servidor') }}</h2>

                @if ($this->viewerMember?->role === 'owner')
                    <form wire:submit="updateServer" class="mt-5">
                        <label class="text-xs font-bold uppercase tracking-wide text-[#b5bac1]">{{ __('Nome do servidor') }}</label>
                        <div class="mt-2 flex gap-2">
                            <input wire:model="serverName" type="text" class="w-full rounded border-0 bg-[#1e1f22] px-3 py-2.5 text-white focus:ring-0">
                            <button type="submit" class="shrink-0 cursor-pointer rounded bg-[#5865f2] px-4 text-sm font-medium text-white hover:bg-[#4752c4]">{{ __('Salvar') }}</button>
                        </div>
                        @error('serverName') <p class="mt-1 text-sm text-[#f23f43]">{{ $message }}</p> @enderror
                    </form>
                @endif

                <div class="mt-6">
                    <p class="text-xs font-bold uppercase tracking-wide text-[#b5bac1]">
                        {{ __('Membros') }} — {{ $this->members->count() }}
                    </p>

                    <div class="mt-2 max-h-64 space-y-1 overflow-y-auto">
                        @foreach ($this->members as $member)
                            <div wire:key="cfg-{{ $member->id }}" class="flex items-center gap-2 rounded px-2 py-1.5 hover:bg-[#2b2d31]">
                                <div class="flex size-7 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-[10px] font-semibold text-white">
                                    @if ($member->user->avatar_url)
                                        <img src="{{ $member->user->avatar_url }}" alt="" class="size-7 object-cover">
                                    @else
                                        {{ $member->user->initials() }}
                                    @endif
                                </div>
                                <span class="min-w-0 flex-1 truncate text-sm text-[#dbdee1]">{{ $member->user->displayName() }}</span>
                                <span class="shrink-0 text-xs {{ $member->role === 'owner' ? 'text-[#f0b232]' : 'text-[#949ba4]' }}">{{ $member->role }}</span>
                            </div>
                        @endforeach
                    </div>
                </div>

                <div class="mt-6 flex items-center justify-between border-t border-[#3f4147] pt-5">
                    @if ($this->viewerMember?->role === 'owner')
                        <button
                            type="button"
                            wire:click="deleteServer"
                            wire:confirm="{{ __('Excluir :nome? Some tudo: canais, mensagens e membros.', ['nome' => $this->currentServer->name]) }}"
                            class="cursor-pointer rounded bg-[#da373c] px-4 py-2 text-sm font-medium text-white hover:bg-[#a12828]"
                        >{{ __('Excluir servidor') }}</button>
                    @else
                        <span></span>
                    @endif

                    <button type="button" wire:click="$set('editingServer', false)" class="cursor-pointer px-4 py-2 text-sm text-white hover:underline">{{ __('Fechar') }}</button>
                </div>
            </div>
        </div>
    @endif

    {{-- Modal: convite --}}
    @if ($this->currentServer)
        <div x-cloak x-show="invite" class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-[2px]" x-on:click.self="invite = false">
            <div class="w-full max-w-sm rounded-lg bg-[#313338] p-6">
                <h2 class="text-xl font-semibold text-white">{{ __('Convidar para :servidor', ['servidor' => $this->currentServer->name]) }}</h2>
                <p class="mt-1 text-sm text-[#b5bac1]">{{ __('Só entra quem estiver logado.') }}</p>

                <div class="mt-5 flex gap-2" x-data="{ copied: false }">
                    <input readonly value="{{ $this->inviteUrl }}" class="w-full rounded border-0 bg-[#1e1f22] px-3 py-2.5 text-sm text-[#dbdee1] focus:ring-0">
                    <button
                        type="button"
                        x-on:click="navigator.clipboard.writeText('{{ $this->inviteUrl }}'); copied = true; setTimeout(() => copied = false, 2000)"
                        class="shrink-0 cursor-pointer rounded bg-[#5865f2] px-4 text-sm font-medium text-white hover:bg-[#4752c4]"
                        x-text="copied ? '{{ __('Copiado') }}' : '{{ __('Copiar') }}'"
                    ></button>
                </div>
            </div>
        </div>
    @endif
</div>
