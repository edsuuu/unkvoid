<section class="lp-section" style="min-height:100vh;display:flex;align-items:center;justify-content:center;padding-top:96px;padding-bottom:64px">
    <div style="width:100%;max-width:480px;background:rgba(255,255,255,0.03);border:1px solid rgba(255,255,255,0.08);border-radius:16px;padding:32px;text-align:center">
        @if ($state === 'valid')
            @if ($iconUrl)
                <img src="{{ $iconUrl }}" alt="" width="64" height="64" style="width:64px;height:64px;border-radius:16px;object-fit:cover;margin:0 auto">
            @else
                <div style="width:64px;height:64px;border-radius:16px;margin:0 auto;display:flex;align-items:center;justify-content:center;background:#5865f2;font-size:22px;font-weight:600">{{ mb_strtoupper(mb_substr($serverName, 0, 1)) }}</div>
            @endif

            <p style="color:var(--lp-ink-2);margin:20px 0 0;font-size:15px">
                {{ $inviter ? $inviter.' convidou você para entrar em' : 'Você foi convidado para entrar em' }}
            </p>
            <h1 style="font-size:26px;line-height:1.2;font-weight:600;margin:8px 0 0;overflow-wrap:anywhere">{{ $serverName }}</h1>
            <p style="color:var(--lp-dim);margin:10px 0 0;font-size:14px">{{ $members }} {{ $members === 1 ? 'membro' : 'membros' }}</p>

            <div x-data="{ copied: false }" style="display:flex;flex-direction:column;gap:10px;margin-top:28px">
                <button type="button" class="lp-btn-primary" style="justify-content:center"
                    @click="navigator.clipboard.writeText(@js(route('invite.show', $code))); copied = true; setTimeout(() => copied = false, 2000)">
                    <span x-text="copied ? 'Link copiado' : 'Copiar o convite'">Copiar o convite</span>
                </button>
                <a href="{{ route('home') }}#download" class="lp-btn-ghost" style="justify-content:center" wire:navigate>Baixar o Unkvoid</a>
            </div>

            <p style="color:var(--lp-dim);margin:20px 0 0;font-size:13px;line-height:1.5">
                No app, clique em <strong>+</strong> na barra dos servidores, escolha <strong>Entrar em um servidor</strong> e cole o convite.
            </p>
        @else
            <h1 style="font-size:24px;font-weight:600;margin:0">
                @switch($state)
                    @case('spent') Esse convite venceu @break
                    @case('paused') Os convites deste servidor estão pausados @break
                    @default Esse convite não existe
                @endswitch
            </h1>
            <p style="color:var(--lp-ink-2);margin:12px 0 0;font-size:15px">Peça um convite novo a quem chamou você.</p>
            <div style="margin-top:28px">
                <a href="{{ route('home') }}" class="lp-btn-ghost" wire:navigate>Ir para o início</a>
            </div>
        @endif
    </div>
</section>
