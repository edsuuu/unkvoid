<div class="flex flex-col gap-6">
    <div
        class="relative h-auto w-full"
        x-cloak
        x-data="{
            showRecoveryInput: @js($errors->has('recovery_code')),
            focusOtp() {
                this.$nextTick(() => this.$refs.otp?.querySelector('input')?.focus());
            },
            init() {
                if (! this.showRecoveryInput) {
                    this.focusOtp();
                }
            },
            toggleInput() {
                this.showRecoveryInput = ! this.showRecoveryInput;

                this.$wire.set('code', '');
                this.$wire.set('recovery_code', '');

                this.$nextTick(() => {
                    this.showRecoveryInput
                        ? this.$refs.recovery_code?.focus()
                        : this.focusOtp();
                });
            },
        }"
    >
        <div x-show="! showRecoveryInput">
            <div class="flex w-full flex-col text-center">
                <h1 class="text-2xl font-semibold text-white">{{ __('Authentication code') }}</h1>
                <p class="text-sm text-[#b5bac1]">{{ __('Enter the authentication code provided by your authenticator application.') }}</p>
            </div>
        </div>

        <div x-show="showRecoveryInput">
            <div class="flex w-full flex-col text-center">
                <h1 class="text-2xl font-semibold text-white">{{ __('Recovery code') }}</h1>
                <p class="text-sm text-[#b5bac1]">{{ __('Please confirm access to your account by entering one of your emergency recovery codes.') }}</p>
            </div>
        </div>

        <form wire:submit="login">
            <div class="space-y-5 text-center">
                <div x-show="! showRecoveryInput" class="my-5" x-ref="otp">
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

                <div x-show="showRecoveryInput" class="my-5">
                    <x-forms.input
                        wire:model="recovery_code"
                        name="recovery_code"
                        type="text"
                        x-ref="recovery_code"
                        autocomplete="one-time-code"
                        aria-label="{{ __('Recovery code') }}"
                    />
                </div>

                <x-forms.button variant="primary" type="submit" class="w-full">
                    {{ __('Continue') }}
                </x-forms.button>
            </div>

            <div class="mt-5 space-x-0.5 text-center text-sm leading-5">
                <span class="opacity-50">{{ __('or you can') }}</span>
                <div class="inline cursor-pointer font-medium underline opacity-80">
                    <span x-show="! showRecoveryInput" x-on:click="toggleInput()">{{ __('login using a recovery code') }}</span>
                    <span x-show="showRecoveryInput" x-on:click="toggleInput()">{{ __('login using an authentication code') }}</span>
                </div>
            </div>
        </form>
    </div>
</div>
