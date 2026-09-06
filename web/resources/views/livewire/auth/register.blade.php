<div class="flex flex-col gap-6">
    <div class="flex w-full flex-col text-center">
        <h1 class="text-2xl font-semibold text-white">{{ __('Create an account') }}</h1>
        <p class="text-sm text-[#b5bac1]">{{ __('Enter your details below to create your account') }}</p>
    </div>

    @if (session('status'))
        <div class="text-center text-sm font-medium text-green-600">
            {{ session('status') }}
        </div>
    @endif

    <form wire:submit="register" class="flex flex-col gap-6">
        <x-forms.input
            wire:model="name"
            :label="__('Name')"
            type="text"
            required
            autofocus
            autocomplete="name"
            :placeholder="__('Full name')"
        />

        <x-forms.input
            wire:model="email"
            :label="__('Email address')"
            type="email"
            required
            autocomplete="email"
            placeholder="email@example.com"
        />

        <x-forms.input
            wire:model="password"
            :label="__('Password')"
            type="password"
            required
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
                {{ __('Create account') }}
            </x-forms.button>
        </div>
    </form>

    <x-google-button />

    <div class="space-x-1 text-center text-sm text-zinc-600 rtl:space-x-reverse dark:text-zinc-400">
        <span>{{ __('Already have an account?') }}</span>
        <a href="{{ route('login') }}" wire:navigate class="font-medium text-zinc-900 underline underline-offset-4 hover:text-zinc-700 dark:text-white dark:hover:text-zinc-300">
            {{ __('Log in') }}
        </a>
    </div>
</div>
