<div class="flex flex-col gap-6">
    <div class="flex w-full flex-col text-center">
        <h1 class="text-2xl font-semibold text-white">{{ __('Reset password') }}</h1>
        <p class="text-sm text-[#b5bac1]">{{ __('Enter your new password below') }}</p>
    </div>

    <form wire:submit="resetPassword" class="flex flex-col gap-6">
        <x-forms.input
            wire:model="email"
            :label="__('Email address')"
            type="email"
            required
            readonly
            autocomplete="email"
        />

        <x-forms.input
            wire:model="password"
            :label="__('Password')"
            type="password"
            required
            autofocus
            autocomplete="new-password"
            :placeholder="__('Password')"
            viewable
        />

        <x-forms.input
            wire:model="password_confirmation"
            :label="__('Confirm password')"
            type="password"
            required
            autocomplete="new-password"
            :placeholder="__('Confirm password')"
            viewable
        />

        <div class="flex items-center justify-end">
            <x-forms.button variant="primary" type="submit" class="w-full">
                {{ __('Reset password') }}
            </x-forms.button>
        </div>
    </form>
</div>
