<x-layout :title="__('Reset password')" layout="bare">
    <x-auth-card>
        <livewire:auth.reset-password :token="request()->route('token')" :email="request()->string('email')" />
    </x-auth-card>
</x-layout>
