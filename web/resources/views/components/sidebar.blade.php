<div
    x-show="sidebarOpen"
    x-transition.opacity
    x-cloak
    x-on:click="sidebarOpen = false"
    class="fixed inset-0 z-40 bg-zinc-900/50 lg:hidden"
></div>

<aside
    class="fixed inset-y-0 start-0 z-50 flex w-64 shrink-0 flex-col gap-4 border-e border-zinc-200 bg-zinc-50 p-4 transition-transform duration-200 lg:sticky lg:top-0 lg:h-screen lg:translate-x-0 dark:border-zinc-700 dark:bg-zinc-900"
    x-bind:class="{ '-translate-x-full': ! sidebarOpen }"
>
    <button
        type="button"
        x-on:click="sidebarOpen = false"
        aria-label="{{ __('Close sidebar') }}"
        class="cursor-pointer self-end rounded-md p-1 text-zinc-500 hover:bg-zinc-200 lg:hidden dark:hover:bg-white/10"
    >
        <x-icon name="x-mark" />
    </button>

    <x-app-logo :href="route('home')" class="px-2" wire:navigate />

    @auth
        <nav class="flex flex-col gap-1">
            <a
                href="{{ route('app') }}"
                wire:navigate
                @class([
                    'flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium',
                    'bg-zinc-200 text-zinc-900 dark:bg-white/10 dark:text-white' => request()->routeIs('app'),
                    'text-zinc-700 hover:bg-zinc-200 dark:text-zinc-300 dark:hover:bg-white/10' => ! request()->routeIs('app'),
                ])
            >
                <x-icon name="layout-grid" class="size-4" />
                {{ __('Dashboard') }}
            </a>
        </nav>
    @endauth

    <div class="flex-1"></div>

    @auth
        <x-user-menu position="top" />
    @endauth
</aside>
