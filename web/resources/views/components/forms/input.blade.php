@props([
    'label' => null,
    'type' => 'text',
    'viewable' => false,
])

<div class="grid gap-2" @if ($viewable) x-data="{ visible: false }" @endif>
    <label class="grid gap-2">
        @if (filled($label))
            <span class="text-sm font-medium leading-tight text-zinc-800 dark:text-zinc-200">{{ $label }}</span>
        @endif

        <div class="relative">
            <input
                @if ($viewable)
                    x-bind:type="visible ? 'text' : 'password'"
                @else
                    type="{{ $type }}"
                @endif
                {{ $attributes->class([
                    'w-full rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 shadow-xs placeholder:text-zinc-400 read-only:bg-zinc-50 disabled:opacity-50 dark:border-zinc-600 dark:bg-zinc-900 dark:text-white dark:placeholder:text-zinc-500 dark:read-only:bg-zinc-800',
                    'focus:outline-hidden focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-accent-foreground',
                    'pe-10' => $viewable,
                ]) }}
            />

            @if ($viewable)
                <button
                    type="button"
                    tabindex="-1"
                    @click="visible = ! visible"
                    :aria-label="visible ? '{{ __('Hide password') }}' : '{{ __('Show password') }}'"
                    class="absolute inset-y-0 end-0 flex cursor-pointer items-center px-3 text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
                >
                    <x-icon name="eye" x-show="! visible" class="size-4" />
                    <x-icon name="eye-slash" x-show="visible" x-cloak class="size-4" />
                </button>
            @endif
        </div>
    </label>

    @error($attributes->wire('model')->value() ?: $attributes->get('name', 'none'))
        <p class="text-sm text-red-600 dark:text-red-400">{{ $message }}</p>
    @enderror
</div>
