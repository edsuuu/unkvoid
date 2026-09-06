<section class="mt-10 space-y-6" x-data="{ open: @js($errors->isNotEmpty()) }">
    <div class="relative mb-5">
        <h3 class="text-base font-semibold text-zinc-900 dark:text-white">{{ __('Delete account') }}</h3>
        <p class="text-sm text-zinc-500 dark:text-zinc-400">{{ __('Delete your account and all of its resources') }}</p>
    </div>

    <x-forms.button variant="danger" x-on:click="open = true">
        {{ __('Delete account') }}
    </x-forms.button>

    <div
        x-show="open"
        x-cloak
        x-on:keydown.escape.window="open = false"
        class="fixed inset-0 z-50 flex items-center justify-center p-4"
    >
        <div class="absolute inset-0 bg-zinc-900/50" x-on:click="open = false"></div>

        <div class="relative z-10 w-full max-w-lg rounded-xl border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-700 dark:bg-zinc-900">
            <form method="POST" wire:submit="deleteUser" class="space-y-6">
                <div>
                    <h4 class="text-lg font-semibold text-zinc-900 dark:text-white">{{ __('Are you sure you want to delete your account?') }}</h4>

                    <p class="text-sm text-zinc-500 dark:text-zinc-400">
                        {{ __('Once your account is deleted, all of its resources and data will be permanently deleted. Please enter your password to confirm you would like to permanently delete your account.') }}
                    </p>
                </div>

                <x-forms.input wire:model="password" :label="__('Password')" type="password" viewable />

                <div class="flex justify-end gap-2">
                    <x-forms.button variant="filled" x-on:click="open = false">{{ __('Cancel') }}</x-forms.button>

                    <x-forms.button variant="danger" type="submit">{{ __('Delete account') }}</x-forms.button>
                </div>
            </form>
        </div>
    </div>
</section>
