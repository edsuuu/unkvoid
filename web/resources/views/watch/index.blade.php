<x-guest-layout :title="__('Assistir')">
    @push('scripts')
        @vite('resources/js/watch.js')
    @endpush

    <section class="lp-auth" style="align-items:flex-start" data-watch="{{ request()->route('code') }}">
        <div style="width:min(1120px,100%);margin:0 auto">
            <div class="lp-form-card" data-entry style="margin:0 auto">
                <p class="lp-form-title">Assistir pelo navegador</p>
                <p class="lp-form-sub">Sala <span class="lp-code-chip">{{ request()->route('code') }}</span>. Só precisa de um nome.</p>

                <form data-join>
                    <label class="lp-label" for="name">SEU NOME</label>
                    <input id="name" class="lp-input" data-name type="text" maxlength="40" placeholder="Como aparecer para os outros" required autofocus>
                    <button type="submit" class="lp-btn-block">Entrar na sala</button>
                </form>

                <p class="lp-form-foot" style="display:block;text-align:center">Chrome, Firefox e Edge assistem. Para transmitir, é o app.</p>
            </div>

            <div data-room hidden>
                <div data-stage style="display:grid;gap:12px;grid-template-columns:repeat(auto-fit,minmax(min(100%,480px),1fr))"></div>
                <div data-empty class="lp-form-card" style="margin:0 auto;text-align:center">
                    <p class="lp-form-title">Ninguém está compartilhando ainda.</p>
                    <p class="lp-form-sub" style="margin:0">Quando alguém transmitir, a tela aparece aqui.</p>
                </div>
            </div>

            <p data-status class="lp-error" style="text-align:center;margin-top:14px;color:var(--lp-muted)"></p>

            <template>
                <div style="position:relative;background:#000;border:1px solid rgba(255,255,255,0.09);border-radius:16px;overflow:hidden">
                    <video autoplay playsinline style="display:block;width:100%;aspect-ratio:16/9;background:#000"></video>
                    <audio autoplay></audio>
                    <div style="display:flex;align-items:center;justify-content:space-between;padding:8px 12px;font-size:12.5px;color:var(--lp-muted)">
                        <span data-peer-name></span>
                        <button type="button" data-fullscreen class="lp-copy">Tela cheia</button>
                    </div>
                </div>
            </template>
        </div>
    </section>
</x-guest-layout>
