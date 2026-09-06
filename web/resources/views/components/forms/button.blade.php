@props([
    'variant' => 'primary',
    'type' => 'button',
])

<button
    type="{{ $type }}"
    {{ $attributes->class([
        'inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition disabled:pointer-events-none disabled:opacity-50',
        'focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-accent-foreground',
        'bg-zinc-900 text-white hover:bg-zinc-800 dark:bg-white dark:text-zinc-900 dark:hover:bg-zinc-200' => $variant === 'primary',
        'bg-red-600 text-white hover:bg-red-500' => $variant === 'danger',
        'bg-zinc-100 text-zinc-900 hover:bg-zinc-200 dark:bg-white/10 dark:text-white dark:hover:bg-white/15' => $variant === 'filled',
        'border border-zinc-300 text-zinc-900 hover:bg-zinc-50 dark:border-zinc-600 dark:text-white dark:hover:bg-white/5' => $variant === 'outline',
        'text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white' => $variant === 'ghost',
    ]) }}
>
    {{ $slot }}
</button>
