<div class="space-y-8" wire:poll.3s>
    <div>
        <flux:heading size="xl">{{ __('Rede do servidor') }}</flux:heading>
        <flux:subheading>{{ __('O que está passando pelas placas de rede desta máquina agora: a mídia do SFU, o site e tudo o mais. A conta é a diferença entre duas leituras, e a tela se atualiza sozinha a cada 3 segundos.') }}</flux:subheading>
    </div>

    @if (! $available)
        <p class="py-6 text-center text-zinc-500">{{ __('Esta máquina não expõe contadores de rede (só Linux). Na VPS a página mostra os números.') }}</p>
    @elseif (count($interfaces) === 0)
        <p class="py-6 text-center text-zinc-500">{{ __('Primeira leitura feita. Os números aparecem na próxima atualização.') }}</p>
    @else
        <div class="grid gap-4 sm:grid-cols-2">
            <div class="rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
                <p class="text-xs uppercase tracking-wide text-zinc-500">{{ __('Entrando') }}</p>
                <p class="mt-1 text-3xl font-semibold">{{ number_format($downloadMbps, 2, ',', '.') }} <span class="text-base font-normal text-zinc-500">Mbps</span></p>
            </div>
            <div class="rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
                <p class="text-xs uppercase tracking-wide text-zinc-500">{{ __('Saindo') }}</p>
                <p class="mt-1 text-3xl font-semibold">{{ number_format($uploadMbps, 2, ',', '.') }} <span class="text-base font-normal text-zinc-500">Mbps</span></p>
            </div>
        </div>

        <div class="overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-200 text-left text-xs uppercase tracking-wide text-zinc-500 dark:border-zinc-700">
                        <th class="py-2 pr-4">{{ __('Placa') }}</th>
                        <th class="py-2 pr-4">{{ __('Entrando') }}</th>
                        <th class="py-2 pr-4">{{ __('Saindo') }}</th>
                        <th class="py-2 pr-4">{{ __('Entrou desde o boot') }}</th>
                        <th class="py-2">{{ __('Saiu desde o boot') }}</th>
                    </tr>
                </thead>
                <tbody>
                    @foreach ($interfaces as $interface)
                        <tr class="border-b border-zinc-100 dark:border-zinc-800" wire:key="nic-{{ $interface['name'] }}">
                            <td class="py-2 pr-4 font-mono">{{ $interface['name'] }}</td>
                            <td class="py-2 pr-4 whitespace-nowrap">{{ number_format($interface['receivedMbps'], 2, ',', '.') }} Mbps</td>
                            <td class="py-2 pr-4 whitespace-nowrap">{{ number_format($interface['sentMbps'], 2, ',', '.') }} Mbps</td>
                            <td class="py-2 pr-4 whitespace-nowrap text-zinc-500">{{ number_format($interface['receivedTotal'] / 1_000_000_000, 1, ',', '.') }} GB</td>
                            <td class="py-2 whitespace-nowrap text-zinc-500">{{ number_format($interface['sentTotal'] / 1_000_000_000, 1, ',', '.') }} GB</td>
                        </tr>
                    @endforeach
                </tbody>
            </table>
        </div>
    @endif
</div>
