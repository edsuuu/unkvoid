@props([
    'label' => null,
])

<div class="grid gap-2">
    <label class="flex items-center gap-2">
        <input
            type="checkbox"
            {{ $attributes->class([
                'size-4 rounded border-zinc-300 text-accent-content shadow-xs dark:border-zinc-600 dark:bg-zinc-900',
                'focus:outline-hidden focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-accent-foreground',
            ]) }}
        />

        @if (filled($label))
            <span class="text-sm font-medium leading-tight text-zinc-800 dark:text-zinc-200">{{ $label }}</span>
        @endif
    </label>

    @error($attributes->wire('model')->value() ?: $attributes->get('name', 'none'))
        <p class="text-sm text-red-600 dark:text-red-400">{{ $message }}</p>
    @enderror
</div>
