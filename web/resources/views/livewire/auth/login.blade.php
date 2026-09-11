<div class="lp-form-card">
    <p class="lp-form-title">Entrar</p>
    <p class="lp-form-sub">O login é opcional. Dá para usar tudo sem conta.</p>

    @if (session('status'))
        <p class="lp-notice">{{ session('status') }}</p>
    @endif

    <a href="{{ route('oauth2.google') }}" class="lp-btn-google">
        <x-google-icon />
        Entrar com Google
    </a>

    <div class="lp-mock-or"><i></i><span>OU</span><i></i></div>

    <form wire:submit="login">
        <label class="lp-label" for="email">E-MAIL</label>
        <input wire:model="email" id="email" type="email" autocomplete="email" placeholder="voce@email.com" required autofocus @class(['lp-input', 'is-invalid' => $errors->has('email')])>
        @error('email') <p class="lp-error">{{ $message }}</p> @enderror

        <label class="lp-label" for="password" style="display:flex;justify-content:space-between;align-items:baseline">SENHA <a href="{{ route('password.request') }}" style="color:var(--lp-lilac);letter-spacing:0;text-transform:none;font-family:Archivo,sans-serif;font-size:12px" wire:navigate>Esqueci a senha</a></label>
        <input wire:model="password" id="password" type="password" autocomplete="current-password" placeholder="••••••••" required @class(['lp-input', 'is-invalid' => $errors->has('password')])>
        @error('password') <p class="lp-error">{{ $message }}</p> @enderror

        <button type="submit" class="lp-btn-block" wire:loading.attr="disabled">Entrar</button>
    </form>

    <div class="lp-form-foot">
        <span>Não tem conta?</span>
        <a href="{{ route('register') }}" wire:navigate>Criar conta</a>
        <span style="color:#3a3350">·</span>
        <a href="{{ route('home') }}" style="color:var(--lp-muted-2)">Continuar sem login</a>
    </div>
</div>
