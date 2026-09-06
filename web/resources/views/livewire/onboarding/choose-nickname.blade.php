<div class="flex min-h-screen items-center justify-center bg-[#313338] px-4">
    <div class="w-full max-w-md rounded-lg bg-[#2b2d31] p-8">
        <h1 class="text-2xl font-semibold text-white">{{ __('Quase lá') }}</h1>
        <p class="mt-1 text-sm text-[#b5bac1]">{{ __('Escolha como você aparece para os outros.') }}</p>

        <form wire:submit="save" class="mt-6 space-y-5">
            <div>
                <label class="text-xs font-bold uppercase tracking-wide text-[#b5bac1]">{{ __('Nome') }}</label>
                <input wire:model="name" type="text" class="mt-2 w-full rounded border-0 bg-[#1e1f22] px-3 py-2.5 text-white focus:ring-0">
                @error('name') <p class="mt-1 text-sm text-[#f23f43]">{{ $message }}</p> @enderror
            </div>

            <div>
                <label class="text-xs font-bold uppercase tracking-wide text-[#b5bac1]">{{ __('Nickname') }}</label>
                <div class="mt-2 flex items-center rounded bg-[#1e1f22] px-3">
                    <span class="text-[#6d6f78]">@</span>
                    <input wire:model="nickname" type="text" class="w-full border-0 bg-transparent px-1 py-2.5 text-white focus:ring-0">
                </div>
                @error('nickname') <p class="mt-1 text-sm text-[#f23f43]">{{ $message }}</p> @enderror
            </div>

            <button type="submit" class="w-full rounded bg-[#5865f2] py-2.5 font-medium text-white hover:bg-[#4752c4]">{{ __('Continuar') }}</button>
        </form>
    </div>
</div>
