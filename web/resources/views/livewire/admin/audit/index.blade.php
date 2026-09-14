<div class="space-y-8">
    <div>
        <flux:heading size="xl">{{ __('Auditoria') }}</flux:heading>
        <flux:subheading>{{ __('Quem entrou em qual canal de voz, de onde, e o que mudou nos servidores.') }}</flux:subheading>
    </div>

    <div class="flex gap-2">
        <flux:button size="sm" :variant="$isAccesses ? 'primary' : 'ghost'" wire:click="showTab('accesses')">{{ __('Acessos') }}</flux:button>
        <flux:button size="sm" :variant="$isAccesses ? 'ghost' : 'primary'" wire:click="showTab('changes')">{{ __('Alterações') }}</flux:button>
    </div>

    <div class="grid gap-4 sm:grid-cols-4">
        @if ($isAccesses)
            <flux:select wire:model.live="serverId" :label="__('Servidor')">
                <flux:select.option value="">{{ __('Todos') }}</flux:select.option>
                @foreach ($servers as $id => $name)
                    <flux:select.option value="{{ $id }}">{{ $name }}</flux:select.option>
                @endforeach
            </flux:select>
        @endif
        <flux:input wire:model.live.debounce.400ms="email" :label="__('E-mail do usuário')" placeholder="fulano@" />
        <flux:input type="date" wire:model.live="from" :label="__('De')" />
        <flux:input type="date" wire:model.live="until" :label="__('Até')" />
    </div>

    <div class="overflow-x-auto">
        @if ($isAccesses)
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-200 text-left text-xs uppercase tracking-wide text-zinc-500 dark:border-zinc-700">
                        <th class="py-2 pr-4">{{ __('Entrou') }}</th>
                        <th class="py-2 pr-4">{{ __('Saiu') }}</th>
                        <th class="py-2 pr-4">{{ __('Usuário') }}</th>
                        <th class="py-2 pr-4">{{ __('Servidor') }}</th>
                        <th class="py-2 pr-4">{{ __('Canal') }}</th>
                        <th class="py-2 pr-4">IP</th>
                        <th class="py-2 pr-4">IP (SFU)</th>
                        <th class="py-2">{{ __('Navegador') }}</th>
                    </tr>
                </thead>
                <tbody>
                    @forelse ($accesses as $access)
                        <tr class="border-b border-zinc-100 dark:border-zinc-800" wire:key="access-{{ $access['id'] }}">
                            <td class="py-2 pr-4 whitespace-nowrap">{{ $access['joinedAt'] }}</td>
                            <td class="py-2 pr-4 whitespace-nowrap">
                                <span @class(['text-emerald-600 dark:text-emerald-400' => $access['open']])>{{ $access['leftAt'] ?? __('em chamada') }}</span>
                            </td>
                            <td class="py-2 pr-4">{{ $access['user'] }}</td>
                            <td class="py-2 pr-4">{{ $access['server'] }}</td>
                            <td class="py-2 pr-4">{{ $access['channel'] }}</td>
                            <td class="py-2 pr-4 font-mono">{{ $access['ip'] }}</td>
                            <td class="py-2 pr-4 font-mono">{{ $access['sfuIp'] ?? '—' }}</td>
                            <td class="py-2 max-w-xs truncate text-xs text-zinc-500" title="{{ $access['userAgent'] }}">{{ $access['userAgent'] ?? '—' }}</td>
                        </tr>
                    @empty
                        <tr><td colspan="8" class="py-6 text-center text-zinc-500">{{ __('Nenhum acesso registrado.') }}</td></tr>
                    @endforelse
                </tbody>
            </table>
        @else
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-200 text-left text-xs uppercase tracking-wide text-zinc-500 dark:border-zinc-700">
                        <th class="py-2 pr-4">{{ __('Quando') }}</th>
                        <th class="py-2 pr-4">{{ __('Usuário') }}</th>
                        <th class="py-2 pr-4">{{ __('Evento') }}</th>
                        <th class="py-2 pr-4">{{ __('Registro') }}</th>
                        <th class="py-2 pr-4">{{ __('Antes') }}</th>
                        <th class="py-2 pr-4">{{ __('Depois') }}</th>
                        <th class="py-2">IP</th>
                    </tr>
                </thead>
                <tbody>
                    @forelse ($changes as $change)
                        <tr class="border-b border-zinc-100 dark:border-zinc-800" wire:key="change-{{ $change['id'] }}">
                            <td class="py-2 pr-4 whitespace-nowrap">{{ $change['when'] }}</td>
                            <td class="py-2 pr-4">{{ $change['user'] }}</td>
                            <td class="py-2 pr-4">{{ $change['event'] }}</td>
                            <td class="py-2 pr-4 font-mono text-xs">{{ $change['model'] }}</td>
                            <td class="py-2 pr-4 font-mono text-xs break-all">{{ $change['before'] }}</td>
                            <td class="py-2 pr-4 font-mono text-xs break-all">{{ $change['after'] }}</td>
                            <td class="py-2 font-mono">{{ $change['ip'] ?? '—' }}</td>
                        </tr>
                    @empty
                        <tr><td colspan="7" class="py-6 text-center text-zinc-500">{{ __('Nenhuma alteração registrada.') }}</td></tr>
                    @endforelse
                </tbody>
            </table>
        @endif
    </div>

    @if ($canLoadMore)
        <div class="flex justify-center">
            <flux:button size="sm" variant="ghost" wire:click="loadMore">{{ __('Carregar mais') }}</flux:button>
        </div>
    @endif
</div>
