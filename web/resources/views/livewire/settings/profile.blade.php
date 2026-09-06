<x-settings.layout
    :title="__('Profile settings')"
    :heading="__('Profile')"
    :subheading="__('Update your name and email address')"
>
    <form wire:submit="updateProfileInformation" class="my-6 w-full space-y-6">
        <x-forms.input wire:model="name" :label="__('Name')" type="text" required autofocus autocomplete="name" />

        <div>
            <x-forms.input wire:model="email" :label="__('Email')" type="email" required autocomplete="email" />

            @if ($this->hasUnverifiedEmail)
                <p class="mt-4 text-sm text-zinc-600 dark:text-zinc-400">
                    {{ __('Your email address is unverified.') }}

                    <button
                        type="button"
                        wire:click="resendVerificationNotification"
                        class="cursor-pointer font-medium text-zinc-900 underline underline-offset-4 dark:text-white"
                    >
                        {{ __('Click here to re-send the verification email.') }}
                    </button>
                </p>
            @endif
        </div>

        <div class="flex items-center gap-4">
            <x-forms.button variant="primary" type="submit">{{ __('Save') }}</x-forms.button>
        </div>
    </form>

    @if ($this->showDeleteUser)
        <livewire:settings.delete-user-form />
    @endif
</x-settings.layout>
