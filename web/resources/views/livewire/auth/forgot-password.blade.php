<div class="lp-form-card">
    <p class="lp-form-title">Esqueci a senha</p>
    <p class="lp-form-sub">Diga o e-mail da conta e a gente manda um link para escolher outra.</p>

    @if ($sent)
        <p class="lp-notice">Se esse e-mail tiver conta, o link já está a caminho. Vale por 60 minutos.</p>
    @else
        <form wire:submit="send">
            <label class="lp-label" for="email">E-MAIL</label>
            <input wire:model="email" id="email" type="email" autocomplete="email" placeholder="voce@email.com" required autofocus @class(['lp-input', 'is-invalid' => $errors->has('email')])>
            @error('email') <p class="lp-error">{{ $message }}</p> @enderror

            <button type="submit" class="lp-btn-block" wire:loading.attr="disabled">Enviar o link</button>
        </form>
    @endif

    <div class="lp-form-foot">
        <a href="{{ route('login') }}" wire:navigate>Voltar para entrar</a>
    </div>
</div>
