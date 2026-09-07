<x-settings.layout
    :title="__('Security settings')"
    :heading="__('Update password')"
    :subheading="__('Ensure your account is using a long, random password to stay secure')"
>
    <form method="POST" wire:submit="updatePassword" class="mt-6 space-y-6">
        <x-forms.input
            wire:model="current_password"
            :label="__('Current password')"
            type="password"
            required
            autocomplete="current-password"
            viewable
        />

        <x-forms.input
            wire:model="password"
            :label="__('New password')"
            type="password"
            required
            autocomplete="new-password"
            viewable
        />

        <x-forms.input
            wire:model="password_confirmation"
            :label="__('Confirm password')"
            type="password"
            required
            autocomplete="new-password"
            viewable
        />

        <div class="flex items-center gap-4">
            <x-forms.button variant="primary" type="submit" data-test="update-password-button">{{ __('Save') }}</x-forms.button>
        </div>
    </form>

    @if ($canManageTwoFactor)
        <section class="mt-12">
            <h3 class="text-base font-semibold text-zinc-900 dark:text-white">{{ __('Two-factor authentication') }}</h3>
            <p class="text-sm text-zinc-500 dark:text-zinc-400">{{ __('Manage your two-factor authentication settings') }}</p>

            <div class="mx-auto mt-5 flex w-full flex-col space-y-6 text-sm">
                @if ($twoFactorEnabled)
                    <div class="space-y-4">
                        <p class="text-sm text-zinc-600 dark:text-zinc-400">
                            {{ __('You will be prompted for a secure, random pin during login, which you can retrieve from the TOTP-supported application on your phone.') }}
                        </p>

                        <div class="flex justify-start">
                            <x-forms.button variant="danger" wire:click="disable">
                                {{ __('Disable 2FA') }}
                            </x-forms.button>
                        </div>

                        <livewire:settings.two-factor.recovery-codes :$requiresConfirmation />
                    </div>
                @else
                    <div class="space-y-4">
                        <p class="text-sm text-zinc-500 dark:text-zinc-400">
                            {{ __('When you enable two-factor authentication, you will be prompted for a secure pin during login. This pin can be retrieved from a TOTP-supported application on your phone.') }}
                        </p>

                        <x-forms.button variant="primary" wire:click="enable">
                            {{ __('Enable 2FA') }}
                        </x-forms.button>
                    </div>
                @endif
            </div>
        </section>

        <div
            x-data
            x-show="$wire.showModal"
            x-cloak
            x-on:keydown.escape.window="$wire.closeModal()"
            class="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto p-4"
        >
            <div class="absolute inset-0 bg-zinc-900/50" x-on:click="$wire.closeModal()"></div>

            <div class="relative z-10 w-full max-w-md rounded-xl border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-700 dark:bg-zinc-900">
                <button
                    type="button"
                    x-on:click="$wire.closeModal()"
                    aria-label="{{ __('Close') }}"
                    class="absolute end-4 top-4 cursor-pointer text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
                >
                    <x-icon name="x-mark" />
                </button>

                <div class="space-y-6">
                    <div class="flex flex-col items-center space-y-4">
                        <div class="w-auto rounded-full border border-stone-100 bg-white p-0.5 shadow-sm dark:border-stone-600 dark:bg-stone-800">
                            <div class="relative overflow-hidden rounded-full border border-stone-200 bg-stone-100 p-2.5 dark:border-stone-600 dark:bg-stone-200">
                                <div class="absolute inset-0 flex h-full w-full items-stretch justify-around divide-x divide-stone-200 opacity-50 [&>div]:flex-1 dark:divide-stone-300">
                                    @for ($i = 1; $i <= 5; $i++)
                                        <div></div>
                                    @endfor
                                </div>

                                <div class="absolute inset-0 flex h-full w-full flex-col items-stretch justify-around divide-y divide-stone-200 opacity-50 [&>div]:flex-1 dark:divide-stone-300">
                                    @for ($i = 1; $i <= 5; $i++)
                                        <div></div>
                                    @endfor
                                </div>

                                <x-icon name="qr-code" class="relative z-20 size-6 text-stone-800" />
                            </div>
                        </div>

                        <div class="space-y-2 text-center">
                            <h4 class="text-lg font-semibold text-zinc-900 dark:text-white">{{ $this->modalConfig['title'] }}</h4>
                            <p class="text-sm text-zinc-600 dark:text-zinc-400">{{ $this->modalConfig['description'] }}</p>
                        </div>
                    </div>

                    @if ($showVerificationStep)
                        <div class="space-y-6">
                            <div
                                class="flex flex-col items-center justify-center space-y-3"
                                x-data
                                x-init="$nextTick(() => $el.querySelector('input')?.focus())"
                            >
                                <x-forms.input
                                    wire:model="code"
                                    name="code"
                                    type="text"
                                    inputmode="numeric"
                                    maxlength="6"
                                    autocomplete="one-time-code"
                                    aria-label="{{ __('Authentication code') }}"
                                    class="text-center text-2xl tracking-[0.5em]"
                                />
                            </div>

                            <div class="flex items-center gap-3">
                                <x-forms.button variant="outline" class="flex-1" wire:click="resetVerification">
                                    {{ __('Back') }}
                                </x-forms.button>

                                <x-forms.button
                                    variant="primary"
                                    class="flex-1"
                                    wire:click="confirmTwoFactor"
                                    x-bind:disabled="$wire.code.length < 6"
                                >
                                    {{ __('Confirm') }}
                                </x-forms.button>
                            </div>
                        </div>
                    @else
                        @error('setupData')
                            <div class="flex items-start gap-2 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-400">
                                <x-icon name="x-circle" class="size-5 shrink-0" />
                                {{ $message }}
                            </div>
                        @enderror

                        <div class="flex justify-center">
                            <div class="relative aspect-square w-64 overflow-hidden rounded-lg border border-stone-200 dark:border-stone-700">
                                @empty($qrCodeSvg)
                                    <div class="absolute inset-0 flex animate-pulse items-center justify-center bg-white dark:bg-stone-700">
                                        <x-icon name="loading" class="size-6 text-stone-500" />
                                    </div>
                                @else
                                    <div class="flex h-full items-center justify-center p-4">
                                        <div class="rounded bg-white p-3">
                                            {!! $qrCodeSvg !!}
                                        </div>
                                    </div>
                                @endempty
                            </div>
                        </div>

                        <div>
                            <x-forms.button
                                :disabled="$errors->has('setupData')"
                                variant="primary"
                                class="w-full"
                                wire:click="showVerificationIfNecessary"
                            >
                                {{ $this->modalConfig['buttonText'] }}
                            </x-forms.button>
                        </div>

                        <div class="space-y-4">
                            <div class="relative flex w-full items-center justify-center">
                                <div class="absolute inset-0 top-1/2 h-px w-full bg-stone-200 dark:bg-stone-600"></div>
                                <span class="relative bg-white px-2 text-sm text-stone-600 dark:bg-zinc-900 dark:text-stone-400">
                                    {{ __('or, enter the code manually') }}
                                </span>
                            </div>

                            <div
                                class="flex items-center space-x-2"
                                x-data="{
                                    copied: false,
                                    async copy() {
                                        try {
                                            await navigator.clipboard.writeText(@js($manualSetupKey));
                                            this.copied = true;
                                            setTimeout(() => this.copied = false, 1500);
                                        } catch (error) {
                                            console.warn('[WARN] failed to copy the key to the clipboard', error);
                                        }
                                    }
                                }"
                            >
                                <div class="flex w-full items-stretch rounded-xl border border-stone-200 dark:border-stone-700">
                                    @empty($manualSetupKey)
                                        <div class="flex w-full items-center justify-center bg-stone-100 p-3 dark:bg-stone-700">
                                            <x-icon name="loading" class="size-4 text-stone-500" />
                                        </div>
                                    @else
                                        <input
                                            type="text"
                                            readonly
                                            value="{{ $manualSetupKey }}"
                                            class="w-full bg-transparent p-3 text-stone-900 outline-hidden dark:text-stone-100"
                                        />

                                        <button
                                            type="button"
                                            x-on:click="copy()"
                                            aria-label="{{ __('Copy setup key') }}"
                                            class="cursor-pointer border-s border-stone-200 px-3 transition-colors dark:border-stone-600"
                                        >
                                            <x-icon name="document-duplicate" x-show="! copied" class="size-5" />
                                            <x-icon name="check" x-show="copied" x-cloak class="size-5 text-green-500" />
                                        </button>
                                    @endempty
                                </div>
                            </div>
                        </div>
                    @endif
                </div>
            </div>
        </div>
    @endif
</x-settings.layout>
