<div class="flex flex-col gap-6">
    <div class="flex w-full flex-col text-center">
        <h1 class="text-2xl font-semibold text-white">{{ __('Confirm password') }}</h1>
        <p class="text-sm text-[#b5bac1]">{{ __('This is a secure area of the application. Please confirm your password before continuing.') }}</p>
    </div>

    @if (session('status'))
        <div class="text-center text-sm font-medium text-green-600">
            {{ session('status') }}
        </div>
    @endif

    <form wire:submit="confirmPassword" class="flex flex-col gap-6">
        <x-forms.input
            wire:model="password"
            :label="__('Password')"
            type="password"
            required
            autocomplete="current-password"
            autofocus
            viewable
        />

        <div class="flex justify-end">
            <x-forms.button variant="primary" type="submit" class="w-full">
                {{ __('Confirm') }}
            </x-forms.button>
        </div>
    </form>
</div>
