<div
    class="space-y-6 rounded-xl border border-zinc-200 py-6 shadow-sm dark:border-white/10"
    x-data="{ showRecoveryCodes: false }"
>
    <div class="space-y-2 px-6">
        <div class="flex items-center gap-2">
            <x-icon name="lock-closed" class="size-4" />
            <h3 class="text-lg font-semibold text-zinc-900 dark:text-white">{{ __('2FA recovery codes') }}</h3>
        </div>

        <p class="text-sm text-zinc-500 dark:text-zinc-400">
            {{ __('Recovery codes let you regain access if you lose your 2FA device. Store them in a secure password manager.') }}
        </p>
    </div>

    <div class="px-6">
        <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <x-forms.button
                x-show="! showRecoveryCodes"
                variant="primary"
                x-on:click="showRecoveryCodes = true"
                aria-controls="recovery-codes-section"
            >
                <x-icon name="eye" class="size-4" />
                {{ __('View recovery codes') }}
            </x-forms.button>

            <x-forms.button
                x-show="showRecoveryCodes"
                x-cloak
                variant="primary"
                x-on:click="showRecoveryCodes = false"
                aria-controls="recovery-codes-section"
            >
                <x-icon name="eye-slash" class="size-4" />
                {{ __('Hide recovery codes') }}
            </x-forms.button>

            @if (filled($recoveryCodes))
                <x-forms.button
                    x-show="showRecoveryCodes"
                    x-cloak
                    variant="filled"
                    wire:click="regenerateRecoveryCodes"
                >
                    <x-icon name="arrow-path" class="size-4" />
                    {{ __('Regenerate codes') }}
                </x-forms.button>
            @endif
        </div>

        <div
            x-show="showRecoveryCodes"
            x-transition
            x-cloak
            id="recovery-codes-section"
            class="relative overflow-hidden"
            x-bind:aria-hidden="! showRecoveryCodes"
        >
            <div class="mt-3 space-y-3">
                @error('recoveryCodes')
                    <div class="flex items-start gap-2 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-400">
                        <x-icon name="x-circle" class="size-5 shrink-0" />
                        {{ $message }}
                    </div>
                @enderror

                @if (filled($recoveryCodes))
                    <div
                        class="grid gap-1 rounded-lg bg-zinc-100 p-4 font-mono text-sm dark:bg-white/5"
                        role="list"
                        aria-label="{{ __('Recovery codes') }}"
                    >
                        @foreach ($recoveryCodes as $code)
                            <div
                                role="listitem"
                                class="select-text"
                                wire:loading.class="opacity-50 animate-pulse"
                            >
                                {{ $code }}
                            </div>
                        @endforeach
                    </div>

                    <p class="text-xs text-zinc-500 dark:text-zinc-400">
                        {{ __('Each recovery code can be used once to access your account and will be removed after use. If you need more, click Regenerate codes above.') }}
                    </p>
                @endif
            </div>
        </div>
    </div>
</div>
