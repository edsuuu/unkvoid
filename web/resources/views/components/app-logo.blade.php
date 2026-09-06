@props([
    'name' => 'Laravel Starter Kit',
])

<a {{ $attributes->class('flex items-center gap-2') }}>
    <span class="flex aspect-square size-8 shrink-0 items-center justify-center rounded-md bg-accent-content text-accent-foreground">
        <x-app-logo-icon class="size-5 fill-current text-white dark:text-black" />
    </span>

    <span class="truncate text-sm font-semibold text-zinc-900 dark:text-white">{{ $name }}</span>
</a>
