@props([
    'label' => null,
])

<div class="grid gap-2">
    <label class="grid gap-2">
        @if (filled($label))
            <span class="text-sm font-medium leading-tight text-zinc-800 dark:text-zinc-200">{{ $label }}</span>
        @endif

        <select
            {{ $attributes->class([
                'w-full appearance-none rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 shadow-xs disabled:opacity-50 dark:border-zinc-600 dark:bg-zinc-900 dark:text-white',
                'focus:outline-hidden focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-accent-foreground',
            ]) }}
        >
            {{ $slot }}
        </select>
    </label>

    @error($attributes->wire('model')->value() ?: $attributes->get('name', 'none'))
        <p class="text-sm text-red-600 dark:text-red-400">{{ $message }}</p>
    @enderror
</div>
