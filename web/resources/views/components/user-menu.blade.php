@props([
    'position' => 'bottom',
    'align' => 'start',
])

<div
    x-data="{ open: false }"
    x-on:click.outside="open = false"
    x-on:keydown.escape.window="open = false"
    class="relative"
>
    <button
        type="button"
        x-on:click="open = ! open"
        x-bind:aria-expanded="open"
        data-test="sidebar-menu-button"
        class="flex w-full cursor-pointer items-center gap-2 rounded-lg p-1 text-start hover:bg-zinc-100 dark:hover:bg-white/10"
    >
        <span class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-zinc-200 text-xs font-semibold text-zinc-800 dark:bg-white/10 dark:text-white">
            {{ auth()->user()->initials() }}
        </span>

        <span class="min-w-0 flex-1 truncate text-sm font-medium text-zinc-900 dark:text-white">
            {{ auth()->user()->name }}
        </span>

        <x-icon name="chevrons-up-down" class="size-4 text-zinc-500" />
    </button>

    <div
        x-show="open"
        x-transition
        x-cloak
        @class([
            'absolute z-50 w-60 rounded-lg border border-zinc-200 bg-white p-1 shadow-lg dark:border-zinc-700 dark:bg-zinc-900',
            'bottom-full mb-2' => $position === 'top',
            'top-full mt-2' => $position === 'bottom',
            'start-0' => $align === 'start',
            'end-0' => $align === 'end',
        ])
    >
        <div class="flex items-center gap-2 px-2 py-1.5">
            <span class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-zinc-200 text-xs font-semibold text-zinc-800 dark:bg-white/10 dark:text-white">
                {{ auth()->user()->initials() }}
            </span>

            <div class="grid min-w-0 flex-1 text-sm leading-tight">
                <span class="truncate font-medium text-zinc-900 dark:text-white">{{ auth()->user()->name }}</span>
                <span class="truncate text-zinc-500 dark:text-zinc-400">{{ auth()->user()->email }}</span>
            </div>
        </div>

        <div class="my-1 h-px bg-zinc-200 dark:bg-zinc-700"></div>

        <a
            href="{{ route('profile.edit') }}"
            wire:navigate
            class="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-zinc-800 hover:bg-zinc-100 dark:text-zinc-200 dark:hover:bg-white/10"
        >
            <x-icon name="cog" class="size-4" />
            {{ __('Settings') }}
        </a>

        <form method="POST" action="{{ route('logout') }}">
            @csrf

            <button
                type="submit"
                data-test="logout-button"
                class="flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm text-zinc-800 hover:bg-zinc-100 dark:text-zinc-200 dark:hover:bg-white/10"
            >
                <x-icon name="arrow-right-start-on-rectangle" class="size-4" />
                {{ __('Log out') }}
            </button>
        </form>
    </div>
</div>
