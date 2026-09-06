<div class="flex flex-col gap-6">
    <div class="flex w-full flex-col text-center">
        <h1 class="text-2xl font-semibold text-white">{{ __('Forgot password') }}</h1>
        <p class="text-sm text-[#b5bac1]">{{ __('Enter your email to receive a password reset link') }}</p>
    </div>

    @if (session('status'))
        <div class="text-center text-sm font-medium text-green-600">
            {{ session('status') }}
        </div>
    @endif

    <form wire:submit="sendPasswordResetLink" class="flex flex-col gap-6">
        <x-forms.input
            wire:model="email"
            :label="__('Email address')"
            type="email"
            required
            autofocus
            autocomplete="email"
            placeholder="email@example.com"
        />

        <div class="flex items-center justify-end">
            <x-forms.button variant="primary" type="submit" class="w-full">
                {{ __('Email password reset link') }}
            </x-forms.button>
        </div>
    </form>

    <div class="space-x-1 text-center text-sm text-zinc-600 rtl:space-x-reverse dark:text-zinc-400">
        <span>{{ __('Remember your password?') }}</span>
        <a href="{{ route('login') }}" wire:navigate class="font-medium text-zinc-900 underline underline-offset-4 hover:text-zinc-700 dark:text-white dark:hover:text-zinc-300">
            {{ __('Log in') }}
        </a>
    </div>
</div>
