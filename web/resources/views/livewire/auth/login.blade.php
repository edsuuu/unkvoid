<div class="flex flex-col gap-6">
    <div class="flex w-full flex-col text-center">
        <h1 class="text-2xl font-semibold text-white">{{ __('Log in to your account') }}</h1>
        <p class="text-sm text-[#b5bac1]">{{ __('Enter your email and password below to log in') }}</p>
    </div>

    @if (session('status'))
        <div class="text-center text-sm font-medium text-green-600">
            {{ session('status') }}
        </div>
    @endif

    <form wire:submit="login" class="flex flex-col gap-6">
        <x-forms.input
            wire:model="email"
            :label="__('Email address')"
            type="email"
            required
            autofocus
            autocomplete="email"
            placeholder="email@example.com"
        />

        <div class="relative">
            <x-forms.input
                wire:model="password"
                :label="__('Password')"
                type="password"
                required
                autocomplete="current-password"
                :placeholder="__('Password')"
                viewable
            />

            @if (Route::has('password.request'))
                <a
                    href="{{ route('password.request') }}"
                    wire:navigate
                    class="absolute end-0 top-0 text-sm text-zinc-600 underline underline-offset-4 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white"
                >
                    {{ __('Forgot your password?') }}
                </a>
            @endif
        </div>

        <x-forms.checkbox wire:model="remember" :label="__('Remember me')" />

        <div class="flex items-center justify-end">
            <x-forms.button variant="primary" type="submit" class="w-full" data-test="login-button">
                {{ __('Log in') }}
            </x-forms.button>
        </div>
    </form>

    <x-google-button />

    @if (Route::has('register'))
        <div class="space-x-1 text-center text-sm text-zinc-600 rtl:space-x-reverse dark:text-zinc-400">
            <span>{{ __('Don\'t have an account?') }}</span>
            <a href="{{ route('register') }}" wire:navigate class="font-medium text-zinc-900 underline underline-offset-4 hover:text-zinc-700 dark:text-white dark:hover:text-zinc-300">
                {{ __('Sign up') }}
            </a>
        </div>
    @endif
</div>
