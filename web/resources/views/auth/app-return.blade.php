<x-guest-layout :title="__('Voltando para o app')">
    <section class="lp-auth">
        <div class="lp-form-card">
            <p class="lp-form-title">{{ __('Pronto, :name', ['name' => $name]) }}</p>
            <p class="lp-form-sub">{{ __('Estamos devolvendo você para o Unkvoid. Esta aba fecha sozinha em alguns segundos.') }}</p>

            <a href="{{ $link }}" class="lp-btn-block" id="voltar">{{ __('Abrir o Unkvoid') }}</a>

            <div class="lp-form-foot">
                <span id="aviso">{{ __('Se o app não abrir, clique no botão acima.') }}</span>
            </div>
        </div>
    </section>

    @push('head')
        <meta name="robots" content="noindex">
    @endpush

    <script>
        // O esquema dispara de imediato; o botão fica para o navegador que exige um clique.
        // Fechar só depois de 3 s, porque antes disso o sistema ainda pode estar entregando
        // o endereço ao app.
        window.location.href = @json($link);

        setTimeout(() => {
            window.close();
            document.getElementById('aviso').textContent = @json(__('Já pode fechar esta aba.'));
        }, 3000);
    </script>
</x-guest-layout>
