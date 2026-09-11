<div class="space-y-8">
    <div>
        <flux:heading size="xl">{{ __('Versões do app') }}</flux:heading>
        <flux:subheading>{{ __('Cada arquivo vai para o bucket do MinIO. O atualizador lê') }} <a href="{{ $manifestUrl }}" class="underline" target="_blank">{{ $manifestUrl }}</a>.</flux:subheading>
    </div>

    <form wire:submit="publish" class="grid max-w-2xl gap-5">
        <div class="grid gap-5 sm:grid-cols-2">
            <flux:input wire:model="version" :label="__('Versão')" placeholder="0.0.8" required />
            <flux:select wire:model="platform" :label="__('Plataforma')" placeholder="Escolha…">
                @foreach ($platforms as $value => $label)
                    <flux:select.option value="{{ $value }}">{{ $label }}</flux:select.option>
                @endforeach
            </flux:select>
        </div>

        <flux:input type="file" wire:model="file" :label="__('Instalador')" />
        <flux:input type="file" wire:model="signatureFile" :label="__('Assinatura (.sig), para o app se atualizar sozinho')" />
        <flux:textarea wire:model="notes" :label="__('Notas (opcional)')" rows="3" />

        <div wire:loading wire:target="file" class="text-sm text-zinc-500">{{ __('Enviando o arquivo…') }}</div>

        <div>
            <flux:button type="submit" variant="primary" wire:loading.attr="disabled">{{ __('Publicar') }}</flux:button>
        </div>
    </form>

    <div class="overflow-x-auto">
        <table class="w-full text-sm">
            <thead>
                <tr class="border-b border-zinc-200 text-left text-xs uppercase tracking-wide text-zinc-500 dark:border-zinc-700">
                    <th class="py-2 pr-4">{{ __('Versão') }}</th>
                    <th class="py-2 pr-4">{{ __('Plataforma') }}</th>
                    <th class="py-2 pr-4">{{ __('Arquivo') }}</th>
                    <th class="py-2 pr-4">{{ __('Tamanho') }}</th>
                    <th class="py-2 pr-4">{{ __('Assinado') }}</th>
                    <th class="py-2 pr-4">{{ __('Publicado') }}</th>
                    <th class="py-2"></th>
                </tr>
            </thead>
            <tbody>
                @forelse ($releases as $release)
                    <tr class="border-b border-zinc-100 dark:border-zinc-800" wire:key="release-{{ $release['id'] }}">
                        <td class="py-2 pr-4 font-mono">{{ $release['version'] }}</td>
                        <td class="py-2 pr-4">{{ $release['platform'] }}</td>
                        <td class="py-2 pr-4"><a href="{{ $release['url'] }}" class="underline">{{ $release['fileName'] }}</a></td>
                        <td class="py-2 pr-4">{{ $release['size'] }}</td>
                        <td class="py-2 pr-4">{{ $release['signed'] ? 'sim' : 'não' }}</td>
                        <td class="py-2 pr-4">{{ $release['publishedAt'] }}</td>
                        <td class="py-2 text-right">
                            <flux:button size="sm" variant="danger" wire:click="remove({{ $release['id'] }})" wire:confirm="Apagar esta versão do bucket?">{{ __('Apagar') }}</flux:button>
                        </td>
                    </tr>
                @empty
                    <tr><td colspan="7" class="py-6 text-center text-zinc-500">{{ __('Nenhuma versão publicada ainda.') }}</td></tr>
                @endforelse
            </tbody>
        </table>
    </div>
</div>
