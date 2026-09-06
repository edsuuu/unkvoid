<div class="mt-4 flex flex-col gap-6">
    <p class="text-center text-sm text-[#b5bac1]">
        {{ __('Please verify your email address by clicking on the link we just emailed to you.') }}
    </p>

    @if (session('status') === 'verification-link-sent')
        <p class="text-center text-sm font-medium text-green-600 dark:text-green-400">
            {{ __('A new verification link has been sent to the email address you provided during registration.') }}
        </p>
    @endif

    <div class="flex flex-col items-center justify-between space-y-3">
        <x-forms.button wire:click="sendVerification" variant="primary" class="w-full">
            {{ __('Resend verification email') }}
        </x-forms.button>

        <x-forms.button wire:click="logout" variant="ghost" class="text-sm" data-test="logout-button">
            {{ __('Log out') }}
        </x-forms.button>
    </div>
</div>
