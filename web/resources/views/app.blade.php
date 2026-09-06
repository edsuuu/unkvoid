<x-layout :title="__('Canais')" layout="bare">
    <livewire:workspace.shell
        :server="request()->route('server')"
        :channel="request()->route('channel')"
    />
</x-layout>
