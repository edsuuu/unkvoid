<div class="space-y-4">
    <div class="flex items-center gap-3">
        <span class="h-px flex-1 bg-zinc-300 dark:bg-zinc-700"></span>
        <span class="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{{ __('ou') }}</span>
        <span class="h-px flex-1 bg-zinc-300 dark:bg-zinc-700"></span>
    </div>

    <a
        href="{{ route('google.redirect') }}"
        class="flex w-full items-center justify-center gap-3 rounded-lg border border-zinc-300 px-4 py-2.5 text-sm font-medium text-zinc-900 transition hover:bg-zinc-50 dark:border-zinc-700 dark:text-white dark:hover:bg-white/5"
    >
        <svg class="size-5" viewBox="0 0 24 24" aria-hidden="true">
            <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.76h3.57c2.08-1.92 3.27-4.74 3.27-8.09z"/>
            <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.76c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84A11 11 0 0 0 12 23z"/>
            <path fill="#FBBC05" d="M5.84 14.11a6.6 6.6 0 0 1 0-4.22V7.05H2.18a11 11 0 0 0 0 9.9l3.66-2.84z"/>
            <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1a11 11 0 0 0-9.82 6.05l3.66 2.84C6.71 7.29 9.14 5.38 12 5.38z"/>
        </svg>
        {{ __('Continuar com o Google') }}
    </a>
</div>
