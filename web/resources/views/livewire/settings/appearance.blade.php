<x-settings.layout
    :title="__('Appearance settings')"
    :heading="__('Appearance')"
    :subheading="__('Update the appearance settings for your account')"
>
    <div
        x-data="{ appearance: window.appearance.current }"
        role="group"
        aria-label="{{ __('Appearance') }}"
        class="inline-flex gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-white/10"
    >
        <button
            type="button"
            x-on:click="appearance = 'light'; window.appearance.set('light')"
            x-bind:class="appearance === 'light' ? 'bg-white shadow-xs dark:bg-zinc-700' : ''"
            class="flex cursor-pointer items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium"
        >
            <x-icon name="sun" class="size-4" />
            {{ __('Light') }}
        </button>

        <button
            type="button"
            x-on:click="appearance = 'dark'; window.appearance.set('dark')"
            x-bind:class="appearance === 'dark' ? 'bg-white shadow-xs dark:bg-zinc-700' : ''"
            class="flex cursor-pointer items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium"
        >
            <x-icon name="moon" class="size-4" />
            {{ __('Dark') }}
        </button>

        <button
            type="button"
            x-on:click="appearance = 'system'; window.appearance.set('system')"
            x-bind:class="appearance === 'system' ? 'bg-white shadow-xs dark:bg-zinc-700' : ''"
            class="flex cursor-pointer items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium"
        >
            <x-icon name="computer-desktop" class="size-4" />
            {{ __('System') }}
        </button>
    </div>
</x-settings.layout>
