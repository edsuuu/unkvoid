<div class="space-y-8">
    <div>
        <flux:heading size="xl">{{ __('Erros dos apps instalados') }}</flux:heading>
        <flux:subheading>{{ __('O app manda o trecho do log quando abre depois de um erro. A mesma falha em vários computadores vira uma linha só, com o contador subindo. O endereço é') }} <span class="font-mono">{{ $endpoint }}</span>.</flux:subheading>
    </div>

    <flux:select wire:model.live="platform" :label="__('Sistema')" class="max-w-xs">
        <flux:select.option value="">{{ __('Todos') }}</flux:select.option>
        @foreach ($platforms as $value => $label)
            <flux:select.option value="{{ $value }}">{{ $label }}</flux:select.option>
        @endforeach
    </flux:select>

    <div class="space-y-4">
        @forelse ($reports as $report)
            <div class="rounded-lg border border-zinc-200 p-4 dark:border-zinc-700" wire:key="error-{{ $report['id'] }}">
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div class="min-w-0 space-y-1">
                        <p class="break-words font-mono text-sm">{{ $report['signature'] }}</p>
                        <p class="text-xs text-zinc-500">
                            {{ $platforms[$report['platform']] ?? $report['platform'] }} ·
                            {{ __('versão') }} {{ $report['version'] }} ·
                            {{ __('de') }} {{ $report['firstSeenAt'] }} {{ __('até') }} {{ $report['lastSeenAt'] }}
                        </p>
                    </div>
                    <div class="flex items-center gap-3">
                        <span @class([
                            'rounded-full px-3 py-1 text-xs font-semibold',
                            'bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-200' => $report['repeated'],
                            'bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300' => ! $report['repeated'],
                        ])>{{ $report['occurrences'] }}x</span>
                        <flux:button size="sm" variant="danger" wire:click="remove({{ $report['id'] }})" wire:confirm="Apagar este erro da lista?">{{ __('Apagar') }}</flux:button>
                    </div>
                </div>

                <details class="mt-3">
                    <summary class="cursor-pointer text-xs text-zinc-500">{{ __('Ver o log do último envio') }}</summary>
                    <pre class="mt-2 max-h-96 overflow-auto rounded bg-zinc-50 p-3 font-mono text-xs whitespace-pre-wrap dark:bg-zinc-900">{{ $report['log'] }}</pre>
                </details>
            </div>
        @empty
            <p class="py-6 text-center text-zinc-500">{{ __('Nenhum erro recebido ainda.') }}</p>
        @endforelse
    </div>
</div>
