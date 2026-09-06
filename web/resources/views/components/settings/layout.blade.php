@props([
    'title',
    'heading',
    'subheading' => null,
    'items' => [
        'profile.edit' => 'Profile',
        'security.edit' => 'Security',
        'appearance.edit' => 'Appearance',
    ],
])

<section class="w-full">
    <div class="relative mb-6 w-full">
        <h1 class="text-2xl font-semibold text-zinc-900 dark:text-white">{{ __('Settings') }}</h1>
        <p class="mb-6 text-zinc-500 dark:text-zinc-400">{{ __('Manage your profile and account settings') }}</p>
        <div class="h-px bg-zinc-200 dark:bg-zinc-700"></div>
    </div>

    <h2 class="sr-only">{{ $title }}</h2>

    <div class="flex items-start max-md:flex-col">
        <nav aria-label="{{ __('Settings') }}" class="me-10 flex w-full flex-col gap-1 pb-4 md:w-[220px]">
            @foreach ($items as $route => $label)
                <a
                    href="{{ route($route) }}"
                    wire:navigate
                    @class([
                        'rounded-lg px-3 py-2 text-sm font-medium',
                        'bg-zinc-100 text-zinc-900 dark:bg-white/10 dark:text-white' => request()->routeIs($route),
                        'text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-white/10' => ! request()->routeIs($route),
                    ])
                >
                    {{ __($label) }}
                </a>
            @endforeach
        </nav>

        <div class="h-px w-full bg-zinc-200 md:hidden dark:bg-zinc-700"></div>

        <div class="flex-1 self-stretch max-md:pt-6">
            <h3 class="text-base font-semibold text-zinc-900 dark:text-white">{{ $heading }}</h3>

            @if (filled($subheading))
                <p class="text-sm text-zinc-500 dark:text-zinc-400">{{ $subheading }}</p>
            @endif

            <div class="mt-5 w-full max-w-lg">
                {{ $slot }}
            </div>
        </div>
    </div>
</section>
