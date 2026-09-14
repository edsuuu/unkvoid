<!DOCTYPE html>
<html lang="{{ str_replace('_', '-', app()->getLocale()) }}">
    <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />

        <title>
            {{ filled($title ?? null) ? $title.' · '.config('app.name') : config('app.name') }}
        </title>

        <link rel="icon" href="/favicon.ico" sizes="any">
        <link rel="icon" href="/favicon.svg" type="image/svg+xml">
        <link rel="apple-touch-icon" href="/apple-touch-icon.png">

        @fonts

        @vite(['resources/css/app.css', 'resources/js/app.js'])
        @fluxAppearance

        @livewireStyles
    </head>
    <body class="min-h-screen bg-white dark:bg-zinc-800">
        <flux:sidebar sticky stashable class="border-r border-zinc-200 bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900">
            <flux:sidebar.toggle class="lg:hidden" icon="x-mark" />

            <a href="{{ route('home') }}" class="flex items-center gap-2 px-2 py-1 font-semibold" wire:navigate><img src="{{ asset('images/unkvoid-mark.png') }}" alt="Unkvoid" width="28" height="28"><span>Unkvoid</span></a>

            <flux:navlist variant="outline">
                <flux:navlist.item icon="layout-grid" :href="route('admin')" :current="request()->routeIs('admin')" wire:navigate>
                    {{ __('Painel') }}
                </flux:navlist.item>
                <flux:navlist.item icon="bug-ant" :href="route('admin.errors')" :current="request()->routeIs('admin.errors')" wire:navigate>
                    {{ __('Erros dos apps') }}
                </flux:navlist.item>
                <flux:navlist.item icon="shield-check" :href="route('admin.audit')" :current="request()->routeIs('admin.audit')" wire:navigate>
                    {{ __('Auditoria') }}
                </flux:navlist.item>
                <flux:navlist.item icon="document-text" :href="route('log-viewer.index')" target="_blank">
                    {{ __('Logs') }}
                </flux:navlist.item>
            </flux:navlist>

            <flux:spacer />

            @auth
                <flux:dropdown position="bottom" align="start">
                    <flux:sidebar.profile
                        :name="auth()->user()->name"
                        :initials="auth()->user()->initials()"
                        icon:trailing="chevrons-up-down"
                    />

                    <flux:menu>
                        <div class="flex items-center gap-2 px-1 py-1.5 text-start text-sm">
                            <flux:avatar :name="auth()->user()->name" :initials="auth()->user()->initials()" />
                            <div class="grid flex-1 text-start text-sm leading-tight">
                                <flux:heading class="truncate">{{ auth()->user()->name }}</flux:heading>
                                <flux:text class="truncate">{{ auth()->user()->email }}</flux:text>
                            </div>
                        </div>
                        <flux:menu.separator />
                        <form method="POST" action="{{ route('logout') }}" class="w-full">
                            @csrf
                            <flux:menu.item as="button" type="submit" icon="arrow-right-start-on-rectangle" class="w-full cursor-pointer">
                                {{ __('Sair') }}
                            </flux:menu.item>
                        </form>
                    </flux:menu>
                </flux:dropdown>
            @endauth
        </flux:sidebar>

        <flux:header class="lg:hidden">
            <flux:sidebar.toggle class="lg:hidden" icon="bars-2" inset="left" />
        </flux:header>

        <flux:main>
            {{ $slot }}
        </flux:main>

        @livewireScripts
        @fluxScripts
    </body>
</html>
