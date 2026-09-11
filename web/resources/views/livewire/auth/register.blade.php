<div class="lp-form-card">
    <p class="lp-form-title">Criar conta</p>
    <p class="lp-form-sub">Só para guardar seu nome e suas preferências.</p>

    <a href="{{ route('oauth2.google') }}" class="lp-btn-google">
        <x-google-icon />
        Criar conta com Google
    </a>

    <div class="lp-mock-or"><i></i><span>OU</span><i></i></div>

    <form wire:submit="register">
        <label class="lp-label" for="name">SEU NOME</label>
        <input wire:model="name" id="name" type="text" autocomplete="name" maxlength="40" placeholder="Como aparece na sala" required autofocus @class(['lp-input', 'is-invalid' => $errors->has('name')])>
        @error('name') <p class="lp-error">{{ $message }}</p> @enderror

        <label class="lp-label" for="email">E-MAIL</label>
        <input wire:model="email" id="email" type="email" autocomplete="email" placeholder="voce@email.com" required @class(['lp-input', 'is-invalid' => $errors->has('email')])>
        @error('email') <p class="lp-error">{{ $message }}</p> @enderror

        <label class="lp-label" for="password">SENHA</label>
        <input wire:model="password" id="password" type="password" autocomplete="new-password" placeholder="••••••••" required @class(['lp-input', 'is-invalid' => $errors->has('password')])>
        @error('password') <p class="lp-error">{{ $message }}</p> @enderror

        <label class="lp-label" for="password_confirmation">CONFIRMAR SENHA</label>
        <input wire:model="password_confirmation" id="password_confirmation" type="password" autocomplete="new-password" placeholder="••••••••" required class="lp-input">

        <button type="submit" class="lp-btn-block" wire:loading.attr="disabled">Criar conta</button>
    </form>

    <div class="lp-form-foot">
        <span>Já tem conta?</span>
        <a href="{{ route('login') }}" wire:navigate>Entrar</a>
        <span style="color:#3a3350">·</span>
        <a href="{{ route('home') }}" style="color:var(--lp-muted-2)">Continuar sem login</a>
    </div>
</div>
