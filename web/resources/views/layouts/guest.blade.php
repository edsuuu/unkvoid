<!DOCTYPE html>
<html lang="{{ str_replace('_', '-', app()->getLocale()) }}" class="dark">
    <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <meta name="color-scheme" content="dark" />

        <title>
            {{ filled($title ?? null) ? $title.' · '.config('app.name') : config('app.name') }}
        </title>

        <link rel="icon" href="/favicon.png" type="image/png">
        <link rel="apple-touch-icon" href="/apple-touch-icon.png">

        @fonts

        @vite(['resources/css/app.css', 'resources/js/app.js'])
        @stack('head')

        @livewireStyles
    </head>
    <body class="lp">
        <div class="lp-glow"></div>

        <div class="lp-content">
            <header class="lp-header">
                <div class="lp-header-inner">
                    <a href="{{ route('home') }}#topo" class="lp-brand">
                        <img src="{{ asset('images/unkvoid-mark.png') }}" alt="Unkvoid" width="26" height="26">
                        <span>Unkvoid</span>
                    </a>

                    <nav class="lp-nav">
                        <a href="{{ route('home') }}#app" class="lp-nav-link">Interface</a>
                        <a href="{{ route('home') }}#pipeline" class="lp-nav-link">Pipeline</a>
                        <a href="{{ route('home') }}#roadmap" class="lp-nav-link">Roadmap</a>
                        @auth
                            <form method="POST" action="{{ route('logout') }}" style="display:contents">
                                @csrf
                                <button type="submit" class="lp-nav-link" style="cursor:pointer;font-family:inherit">Sair</button>
                            </form>
                        @else
                            <a href="{{ route('login') }}" class="lp-nav-link">Entrar</a>
                        @endauth
                        <a href="{{ route('home') }}#download" class="lp-nav-cta">Baixar</a>
                    </nav>
                </div>
            </header>

            {{ $slot }}

            <footer class="lp-footer">
                <div style="display:flex;align-items:center;gap:10px">
                    <img src="{{ asset('images/unkvoid-mark.png') }}" alt="" width="22" height="22" style="opacity:.7">
                    <span class="lp-mono" style="font-size:12px;color:var(--lp-dim)">Compartilhamento de tela para desktop · projeto em desenvolvimento</span>
                </div>
                <div class="lp-footer-links">
                    <a href="{{ route('home') }}#app">Interface</a>
                    <a href="{{ route('home') }}#pipeline">Pipeline</a>
                    <a href="{{ route('home') }}#roadmap">Roadmap</a>
                    <a href="{{ route('home') }}#download">Download</a>
                    <a href="{{ route('privacy') }}">Privacidade</a>
                    <a href="{{ route('terms') }}">Termos</a>
                </div>
            </footer>
        </div>

        @livewireScripts
        @stack('scripts')
    </body>
</html>
