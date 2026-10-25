<x-guest-layout :title="__('Documentação')">
    <livewire:documentation.show :section="request()->route('section')" :page="request()->route('page')" />
</x-guest-layout>
