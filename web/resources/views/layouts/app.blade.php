<!DOCTYPE html>
<html lang="{{ str_replace('_', '-', app()->getLocale()) }}" class="dark">
    <head>
        <meta charset="utf-8" />
        <meta name="csrf-token" content="{{ csrf_token() }}" />

        {{-- Reverb no HTML e não no bundle: os assets são compilados nesta máquina e
             enviados prontos, então uma variável de build viraria "localhost" em
             produção. Lido em tempo de execução, o mesmo bundle serve os dois. --}}
        <meta name="reverb-key" content="{{ config('broadcasting.connections.reverb.key') }}" />
        <meta name="reverb-host" content="{{ config('broadcasting.connections.reverb.options.client_host', request()->getHost()) }}" />
        <meta name="reverb-port" content="{{ config('broadcasting.connections.reverb.options.client_port', request()->getPort()) }}" />
        <meta name="reverb-scheme" content="{{ config('broadcasting.connections.reverb.options.client_scheme', request()->getScheme()) }}" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />

        <title>
            {{ filled($title ?? null) ? $title.' - '.config('app.name', 'Laravel') : config('app.name', 'Laravel') }}
        </title>

        <link rel="icon" href="/favicon.ico" sizes="any">
        <link rel="icon" href="/favicon.svg" type="image/svg+xml">
        <link rel="apple-touch-icon" href="/apple-touch-icon.png">

        @fonts


        @vite(['resources/css/app.css', 'resources/js/app.js'])

        @livewireStyles
    </head>
    <body @class([
        'bg-white text-zinc-900 dark:bg-zinc-800 dark:text-white',
        'h-screen overflow-hidden' => $layout === 'bare',
        'min-h-screen' => $layout !== 'bare',
    ])>
        @if ($layout === 'bare')
            {{ $slot }}
        @elseif ($layout === 'sidebar')
            <div class="flex min-h-screen" x-data="{ sidebarOpen: false }">
                <x-sidebar />

                <div class="flex min-w-0 flex-1 flex-col">
                    <header class="flex items-center gap-3 border-b border-zinc-200 bg-zinc-50 px-4 py-3 lg:hidden dark:border-zinc-700 dark:bg-zinc-900">
                        <button
                            type="button"
                            x-on:click="sidebarOpen = true"
                            aria-label="{{ __('Open sidebar') }}"
                            class="cursor-pointer rounded-md p-1 text-zinc-600 hover:bg-zinc-200 dark:text-zinc-300 dark:hover:bg-white/10"
                        >
                            <x-icon name="bars-2" />
                        </button>

                        <x-app-logo :href="route('home')" wire:navigate />
                    </header>

                    <main class="flex-1 p-6">
                        {{ $slot }}
                    </main>
                </div>
            </div>
        @else
            @if ($layout === 'navbar')
                <header class="sticky top-0 z-30 border-b border-zinc-200 bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900">
                    <div class="mx-auto flex w-full max-w-7xl items-center gap-4 px-6 py-3">
                        <x-app-logo :href="route('home')" wire:navigate />

                        @auth
                            <nav class="flex items-center gap-1 max-lg:hidden">
                                <a
                                    href="{{ route('app') }}"
                                    wire:navigate
                                    @class([
                                        'flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium',
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
                            <x-user-menu align="end" />
                        @endauth

                        @guest
                            <div class="flex items-center gap-4 text-sm font-medium">
                                <a href="{{ route('login') }}" wire:navigate class="text-zinc-700 hover:text-zinc-900 dark:text-zinc-300 dark:hover:text-white">
                                    {{ __('Log in') }}
                                </a>
                                <a href="{{ route('register') }}" wire:navigate class="text-zinc-700 hover:text-zinc-900 dark:text-zinc-300 dark:hover:text-white">
                                    {{ __('Register') }}
                                </a>
                            </div>
                        @endguest
                    </div>
                </header>
            @endif

            <main class="mx-auto w-full max-w-7xl p-6">
                {{ $slot }}
            </main>
        @endif

        <x-toast />

        @livewireScripts
    </body>
</html>
