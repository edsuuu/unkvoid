<x-guest-layout :title="__('Nova senha')">
    <section class="lp-auth">
        <livewire:auth.reset-password :token="request()->route('token')" :email="request()->query('email', '')" />
    </section>
</x-guest-layout>
