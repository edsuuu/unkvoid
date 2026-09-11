<div class="lp-form-card">
    <p class="lp-form-title">Nova senha</p>
    <p class="lp-form-sub">Escolha uma senha nova para {{ $email }}.</p>

    <form wire:submit="save">
        <label class="lp-label" for="email">E-MAIL</label>
        <input wire:model="email" id="email" type="email" autocomplete="email" required @class(['lp-input', 'is-invalid' => $errors->has('email')])>
        @error('email') <p class="lp-error">{{ $message }}</p> @enderror

        <label class="lp-label" for="password">SENHA NOVA</label>
        <input wire:model="password" id="password" type="password" autocomplete="new-password" placeholder="••••••••" required autofocus @class(['lp-input', 'is-invalid' => $errors->has('password')])>
        @error('password') <p class="lp-error">{{ $message }}</p> @enderror

        <label class="lp-label" for="password_confirmation">CONFIRMAR SENHA</label>
        <input wire:model="password_confirmation" id="password_confirmation" type="password" autocomplete="new-password" placeholder="••••••••" required class="lp-input">

        <button type="submit" class="lp-btn-block" wire:loading.attr="disabled">Salvar a senha</button>
    </form>
</div>
