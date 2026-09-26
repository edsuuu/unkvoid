<div x-data="landing({{ Js::from($links) }}, {{ Js::from($aptCommands) }})">
    <canvas class="lp-void" data-void></canvas>

    @push('scripts')
        @vite('resources/js/home.js')
    @endpush

    {{-- Herói --}}
    <section id="topo" class="lp-section" style="padding-top:clamp(110px,17vh,190px);padding-bottom:clamp(56px,9vh,110px);display:flex;flex-direction:column;align-items:center;text-align:center">
        <h1 data-reveal style="font-size:clamp(34px,6.4vw,76px);line-height:1.05;letter-spacing:-0.035em;font-weight:600;margin:0;max-width:17ch;text-wrap:balance">Compartilhe sua tela em qualidade cheia com quem você quiser.</h1>
        <p data-reveal style="font-size:clamp(17px,2.2vw,21px);line-height:1.5;color:var(--lp-ink-2);margin:26px 0 0;max-width:42ch;text-wrap:pretty">Sem conta e sem link de reunião. Um código de 12 caracteres, e quem colar ele vê a sua tela.</p>

        <div data-reveal style="display:flex;flex-wrap:wrap;align-items:center;justify-content:center;gap:12px;margin-top:clamp(28px,4.5vh,42px)">
            <a :href="primaryHref" class="lp-btn-primary" style="font-size:16px;padding:16px 28px;box-shadow:0 16px 46px -16px rgba(140,80,240,0.85)">
                <span class="lp-os"><span x-show="os === 'Windows'"><x-os.windows /></span><span x-show="os === 'macOS'"><x-os.apple /></span><span x-show="os === 'Linux'"><x-os.linux /></span></span>
                <span x-text="primaryLabel">Baixar</span>
            </a>
            <a href="#app" class="lp-btn-ghost">Ver a interface</a>
        </div>

        <div data-reveal style="display:flex;flex-wrap:wrap;justify-content:center;gap:10px;margin-top:20px;font-size:13.5px">
            <template x-for="platform in others" :key="platform">
                <a :href="links[platform] ?? '#download'" class="lp-pill"><span class="lp-os"><span x-show="platform === 'Windows'"><x-os.windows /></span><span x-show="platform === 'macOS'"><x-os.apple /></span><span x-show="platform === 'Linux'"><x-os.linux /></span></span><span x-text="platform"></span></a>
            </template>
        </div>

        <p data-reveal style="font-size:12.5px;color:var(--lp-dim);margin:16px 0 0;max-width:46ch">Ao baixar, você concorda com os <a href="{{ route('terms') }}" class="lp-link" wire:navigate>Termos de uso</a> e a <a href="{{ route('privacy') }}" class="lp-link" wire:navigate>Política de privacidade</a>.</p>

        <div data-reveal style="margin-top:clamp(44px,8vh,88px);width:100%;display:flex;flex-wrap:wrap;justify-content:center;gap:8px">
            <span class="lp-chip">Jogar algo com amigos</span>
            <span class="lp-chip">Programação em par</span>
            <span class="lp-chip">Revisão de design</span>
            <span class="lp-chip">Assistir junto</span>
            <span class="lp-chip">Mostrar um problema para alguém</span>
        </div>
    </section>

    {{-- O app --}}
    <section id="app" class="lp-section">
        <div class="lp-section-head" style="max-width:640px;margin-bottom:clamp(24px,4vh,36px)">
            <p class="lp-eyebrow">O app</p>
            <h2 data-reveal class="lp-h2">Três telas. Nada além disso.</h2>
        </div>

        <div style="display:flex;justify-content:center;gap:6px;flex-wrap:wrap;margin-bottom:22px">
            <button type="button" class="lp-tab" :class="{ 'is-active': screen === 'entrada' }" @click="select('entrada')">Entrada</button>
            <button type="button" class="lp-tab" :class="{ 'is-active': screen === 'sala' }" @click="select('sala')">Sala</button>
            <button type="button" class="lp-tab" :class="{ 'is-active': screen === 'share' }" @click="select('share')">Compartilhar</button>
        </div>

        <div data-reveal data-parallax="34" class="lp-frame">
            <div class="lp-frame-inner">
                <div class="lp-frame-bar">
                    <div class="lp-dots"><i></i><i></i><i></i></div>
                    <span style="font-size:12.5px;font-weight:600;letter-spacing:0.06em;margin-left:6px">UNKVOID</span>
                    <div x-show="screen !== 'entrada'" style="position:absolute;left:50%;transform:translateX(-50%);display:flex;gap:4px;background:rgba(255,255,255,0.03);border-radius:10px;padding:3px">
                        <span style="font-size:11.5px;color:var(--lp-muted-2);border-radius:8px;padding:5px 11px">Home</span>
                        <span style="font-size:11.5px;font-weight:600;color:#0d0616;background:linear-gradient(180deg,#aeb0ff,#8a7cf5);border-radius:8px;padding:5px 11px">Sala ativa</span>
                    </div>
                    <div style="margin-left:auto;display:flex;align-items:center;gap:8px">
                        <span style="width:26px;height:26px;border-radius:50%;background:linear-gradient(180deg,#8a7cf5,#5a3fd6);display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:700;color:#0d0616">E</span>
                        <span style="color:var(--lp-muted-2);font-size:9px">▾</span>
                    </div>
                </div>

                <div class="lp-frame-pane" data-pane>
                    <div x-show="screen === 'entrada'" class="lp-mock" style="width:min(420px,100%)">
                        <p style="text-align:center;font-size:17px;font-weight:600;letter-spacing:-0.01em;margin:0 0 6px">Unkvoid</p>
                        <p style="text-align:center;font-size:13.5px;color:var(--lp-muted);margin:0 0 22px">Compartilhe sua tela com quem você quiser.</p>
                        <p class="lp-mock-label">SEU NOME</p>
                        <div class="lp-mock-input is-focus">Seu nome</div>
                        <div class="lp-mock-primary" style="margin-top:12px;box-shadow:0 14px 34px -16px rgba(140,80,240,0.9)">Criar uma sala</div>
                        <div class="lp-mock-or"><i></i><span>OU</span><i></i></div>
                        <div style="display:flex;gap:8px">
                            <div class="lp-mock-input" style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">Código da sala</div>
                            <div style="flex:none;border:1px solid rgba(255,255,255,0.12);border-radius:12px;padding:12px 16px;font-size:13.5px;font-weight:500">Entrar</div>
                        </div>
                    </div>

                    <div x-show="screen === 'sala'" x-cloak class="lp-room">
                        <div style="display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:10px;background:rgba(255,255,255,0.04);border:1px solid rgba(255,255,255,0.08);border-radius:16px;padding:10px 12px">
                            <div style="display:flex;flex-wrap:wrap;align-items:center;gap:8px">
                                <span style="display:inline-flex;align-items:center;gap:8px;background:rgba(0,0,0,0.35);border-radius:11px;padding:6px 10px">
                                    <span class="lp-mono" style="font-size:10px;letter-spacing:0.12em;color:var(--lp-muted-2)">SALA</span>
                                    <span class="lp-code-chip">k3m9xq2vt7bd</span>
                                    <span style="display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;border:1px solid rgba(255,255,255,0.12);border-radius:7px;position:relative">
                                        <span style="width:8px;height:9px;border:1px solid #a89fc0;border-radius:2px;position:absolute;left:5px;top:4px"></span>
                                        <span style="width:8px;height:9px;border:1px solid #a89fc0;border-radius:2px;position:absolute;left:8px;top:7px;background:rgba(16,13,26,0.9)"></span>
                                    </span>
                                </span>
                                <span style="display:inline-flex;align-items:center;gap:8px;font-size:12px;color:var(--lp-ink-3);background:rgba(255,255,255,0.05);border:1px solid rgba(255,255,255,0.12);border-radius:11px;padding:6px 11px;box-shadow:inset 0 1px 0 rgba(255,255,255,0.06)"><span class="lp-live"><i></i><i></i></span>1 pessoa conectada</span>
                            </div>
                            <div style="display:flex;flex-wrap:wrap;align-items:center;gap:8px">
                                <span style="display:inline-flex;align-items:center;justify-content:center;border:1px solid rgba(255,255,255,0.1);border-radius:11px;padding:9px"><span style="display:grid;grid-template-columns:5px 5px;gap:2px"><span style="width:5px;height:5px;background:#a89fc0"></span><span style="width:5px;height:5px;background:#a89fc0"></span><span style="width:5px;height:5px;background:#a89fc0"></span><span style="width:5px;height:5px;background:#a89fc0"></span></span></span>
                                <span style="display:inline-flex;align-items:center;gap:8px;font-size:12.5px;font-weight:600;color:#fff;background:linear-gradient(180deg,#8a7cf5,#5a3fd6);border-radius:11px;padding:8px 14px"><span style="width:13px;height:9px;border:1.5px solid #0d0616;border-radius:2px"></span>Compartilhar tela</span>
                                <span style="display:inline-flex;align-items:center;gap:7px;font-size:12.5px;color:#9aa0e0;border:1px solid rgba(154,160,224,0.28);border-radius:11px;padding:7px 12px"><span style="font-size:12px">→</span>Sair</span>
                            </div>
                        </div>

                        <div class="lp-mock lp-mock-empty" style="align-self:center;width:min(520px,100%);text-align:center;border-radius:24px;padding:clamp(22px,3.4vw,36px)">
                            <div style="position:relative;width:64px;height:64px;margin:0 auto 18px;border-radius:20px;background:rgba(138,124,245,0.16);border:1px solid rgba(138,124,245,0.32);display:flex;align-items:center;justify-content:center;overflow:hidden">
                                <div class="lp-sweep"></div>
                                <span style="position:relative;display:flex;flex-direction:column;align-items:center;gap:3px">
                                    <span style="width:26px;height:17px;border:2px solid #aeb0ff;border-radius:4px;display:flex;align-items:center;justify-content:center;font-size:11px;color:#aeb0ff;line-height:1">↗</span>
                                    <span style="width:11px;height:2px;border-radius:2px;background:#aeb0ff"></span>
                                </span>
                            </div>
                            <h5 style="font-size:clamp(16px,2.2vw,20px);font-weight:600;letter-spacing:-0.02em;margin:0">Ninguém está compartilhando ainda.</h5>
                            <p style="font-size:13.5px;color:var(--lp-muted);margin:10px 0 20px">Mande o código <span class="lp-code-chip" style="border-radius:6px;padding:2px 7px">k3m9xq2vt7bd</span> para quem você quer aqui.</p>
                            <div style="display:flex;flex-wrap:wrap;justify-content:center;gap:8px">
                                <span style="font-size:13px;font-weight:600;color:#fff;background:linear-gradient(180deg,#8a7cf5,#5a3fd6);border-radius:12px;padding:11px 18px">Iniciar compartilhamento</span>
                                <span style="font-size:13px;color:#e6e1f2;background:rgba(255,255,255,0.06);border:1px solid rgba(255,255,255,0.1);border-radius:12px;padding:11px 18px">Copiar o código</span>
                            </div>
                        </div>
                    </div>

                    <div x-show="screen === 'share'" x-cloak class="lp-mock" style="width:min(560px,100%);border-color:rgba(255,255,255,0.1);padding:clamp(18px,2.6vw,28px)">
                        <h4 style="font-size:19px;font-weight:600;letter-spacing:-0.02em;margin:0">Compartilhar tela</h4>
                        <p style="font-size:13.5px;color:var(--lp-muted);margin:6px 0 18px">Escolha o que a sala vai ver.</p>
                        <div style="display:flex;gap:6px;margin-bottom:16px">
                            <span style="font-size:13px;font-weight:500;color:var(--lp-ink);background:rgba(138,124,245,0.18);border:1px solid rgba(138,124,245,0.35);border-radius:999px;padding:7px 14px">Telas</span>
                            <span style="font-size:13px;color:var(--lp-muted-2);border:1px solid rgba(255,255,255,0.08);border-radius:999px;padding:7px 14px">Aplicativos</span>
                        </div>
                        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,150px),1fr));gap:12px">
                            <div style="border:1px solid rgba(138,124,245,0.45);border-radius:14px;overflow:hidden;background:rgba(255,255,255,0.03)">
                                <div style="aspect-ratio:16/10;background:linear-gradient(140deg,rgba(86,63,214,0.30),rgba(8,6,14,1))"></div>
                                <div style="padding:10px 12px">
                                    <div style="font-size:13.5px;font-weight:500">Tela 1</div>
                                    <div class="lp-mono" style="font-size:11px;color:var(--lp-muted-2);margin-top:3px">3456×2234</div>
                                </div>
                            </div>
                            <div style="border:1px solid rgba(255,255,255,0.09);border-radius:14px;overflow:hidden;background:rgba(255,255,255,0.03)">
                                <div style="aspect-ratio:16/10;background:linear-gradient(140deg,rgba(255,255,255,0.06),rgba(8,6,14,1))"></div>
                                <div style="padding:10px 12px">
                                    <div style="font-size:13.5px;font-weight:500">Tela 2</div>
                                    <div class="lp-mono" style="font-size:11px;color:var(--lp-muted-2);margin-top:3px">1920×1080</div>
                                </div>
                            </div>
                        </div>
                        <div style="display:flex;flex-wrap:wrap;gap:10px 20px;margin-top:18px;font-size:13px;color:var(--lp-ink-3)">
                            <span style="display:inline-flex;align-items:center;gap:8px"><span style="width:16px;height:16px;border-radius:5px;background:linear-gradient(180deg,#8a7cf5,#5a3fd6)"></span>Transmitir o áudio</span>
                            <span style="display:inline-flex;align-items:center;gap:8px"><span style="width:16px;height:16px;border-radius:5px;background:linear-gradient(180deg,#8a7cf5,#5a3fd6)"></span>Sem o áudio do Discord</span>
                        </div>
                        <div style="display:flex;flex-wrap:wrap;align-items:center;gap:12px;margin-top:20px;padding-top:16px;border-top:1px solid rgba(255,255,255,0.08)">
                            <div>
                                <div class="lp-mono" style="font-size:9.5px;letter-spacing:0.12em;color:var(--lp-muted-2);margin-bottom:5px">QUALIDADE</div>
                                <div style="display:flex;align-items:center;gap:10px;border:1px solid rgba(255,255,255,0.12);background:rgba(255,255,255,0.04);border-radius:10px;padding:9px 12px;font-size:13px">1440p<span style="color:var(--lp-muted-2);font-size:9px">▾</span></div>
                            </div>
                            <div>
                                <div class="lp-mono" style="font-size:9.5px;letter-spacing:0.12em;color:var(--lp-muted-2);margin-bottom:5px">FPS</div>
                                <div style="display:flex;align-items:center;gap:10px;border:1px solid rgba(255,255,255,0.12);background:rgba(255,255,255,0.04);border-radius:10px;padding:9px 12px;font-size:13px">60<span style="color:var(--lp-muted-2);font-size:9px">▾</span></div>
                            </div>
                            <div style="margin-left:auto;display:flex;gap:10px">
                                <span style="font-size:13.5px;color:var(--lp-muted-2);padding:10px 14px">Cancelar</span>
                                <span style="font-size:13.5px;font-weight:600;color:#fff;background:linear-gradient(180deg,#8a7cf5,#5a3fd6);border-radius:10px;padding:10px 18px">Transmitir</span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </section>

    {{-- Fluxo --}}
    <section id="fluxo" class="lp-section">
        <div class="lp-section-head">
            <p class="lp-eyebrow">Fluxo descomplicado</p>
            <h2 data-reveal class="lp-h2">Como funciona em 3 passos</h2>
            <p class="lp-lead">Sem link de reunião e sem pedir que ninguém instale nada além do app.</p>
        </div>

        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,260px),1fr));gap:14px">
            @foreach ($steps as $step)
                <div data-reveal class="lp-glass" style="height:100%;display:flex;flex-direction:column">
                    <span class="lp-tag">PASSO {{ $step['number'] }}</span>
                    <h3 style="font-size:18px;font-weight:600;margin:18px 0 8px;letter-spacing:-0.01em">{{ $step['title'] }}</h3>
                    <p style="font-size:14px;line-height:1.6;color:var(--lp-muted);margin:0">{{ $step['text'] }}</p>
                    <div class="lp-card-foot">
                        <span>{{ $step['footLabel'] }}</span>
                        <span style="color:var(--lp-lilac)">{{ $step['footValue'] }}</span>
                    </div>
                </div>
            @endforeach
        </div>
    </section>

    {{-- Pipeline --}}
    <section id="pipeline" class="lp-section">
        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,320px),1fr));gap:clamp(28px,4vw,56px);align-items:center">
            <div>
                <p class="lp-eyebrow">Pipeline de hardware</p>
                <h2 data-reveal class="lp-h2" style="max-width:18ch">Por que a aba do navegador engasga e o app não.</h2>
                <p data-reveal style="font-size:15px;line-height:1.65;color:var(--lp-muted);margin:20px 0 0;max-width:52ch">No navegador, o vídeo é codificado pela CPU, disputando com o resto da máquina. No app, o quadro é codificado uma vez na placa de vídeo e sobe uma vez para o servidor, que replica para quem está assistindo.</p>

                <div style="margin-top:26px;display:flex;flex-direction:column;gap:12px">
                    @foreach ($reasons as $reason)
                        <div data-reveal style="display:flex;gap:16px;align-items:flex-start;padding:16px 2px;border-top:1px solid rgba(255,255,255,0.07)">
                            <div class="lp-num">{{ $reason['number'] }}</div>
                            <div>
                                <h4 style="font-size:14.5px;font-weight:600;margin:0">{{ $reason['title'] }}</h4>
                                <p style="font-size:13px;line-height:1.55;color:var(--lp-muted);margin:5px 0 0">{{ $reason['text'] }}</p>
                            </div>
                        </div>
                    @endforeach
                </div>
            </div>

            <div data-reveal data-parallax="26" class="lp-glass" style="border-radius:22px;padding:clamp(18px,2.6vw,28px);box-shadow:inset 0 1px 0 rgba(255,255,255,0.05),0 30px 80px -40px rgba(0,0,0,0.9)">
                <div style="display:flex;align-items:center;justify-content:space-between;gap:12px;padding-bottom:14px;margin-bottom:16px;border-bottom:1px solid rgba(255,255,255,0.07)">
                    <span class="lp-mono" style="font-size:11px;letter-spacing:0.12em;text-transform:uppercase;color:var(--lp-ink-3)">Caminho do quadro</span>
                    <span class="lp-tag">até 1440p60</span>
                </div>

                <div style="display:flex;flex-direction:column;gap:8px">
                    <div class="lp-path-row"><span>Captura da tela</span><span class="lp-mono" style="font-size:11px;color:var(--lp-dim)">01</span></div>
                    <div class="lp-path-link"></div>
                    <div class="lp-path-row is-gpu"><span>Buffer na GPU</span><span class="lp-code-chip" style="font-size:10.5px;border-radius:6px">GPU</span></div>
                    <div class="lp-path-link"></div>
                    <div class="lp-path-row is-gpu"><span>Encoder de hardware</span><span class="lp-code-chip" style="font-size:10.5px;border-radius:6px">GPU</span></div>
                    <div class="lp-path-link"></div>
                    <div class="lp-path-row"><span>Envio único para o servidor</span><span class="lp-mono" style="font-size:11px;color:var(--lp-dim)">04</span></div>
                    <div class="lp-path-link"></div>
                    <div class="lp-path-row">
                        <span>N espectadores</span>
                        <span style="display:flex;gap:4px">
                            <span style="width:18px;height:12px;border:1px solid rgba(255,255,255,0.22);border-radius:3px"></span>
                            <span style="width:18px;height:12px;border:1px solid rgba(255,255,255,0.22);border-radius:3px"></span>
                            <span style="width:18px;height:12px;border:1px solid rgba(255,255,255,0.22);border-radius:3px"></span>
                        </span>
                    </div>
                </div>

            </div>
        </div>
    </section>

    {{-- Roadmap --}}
    <section id="roadmap" class="lp-section">
        <div class="lp-section-head">
            <p class="lp-eyebrow">Próximos passos</p>
            <h2 data-reveal class="lp-h2">O que vem depois</h2>
            <p class="lp-lead">Nada disso está pronto ainda. É o que está sendo trabalhado para as próximas versões.</p>
        </div>

        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,205px),1fr));gap:12px">
            @foreach ($roadmap as $item)
                <div data-reveal class="lp-glass lp-glass-soft" style="height:100%;display:flex;flex-direction:column">
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:10px">
                        <span class="lp-status"><i></i>Planejado</span>
                        <span class="lp-mono" style="font-size:11px;color:var(--lp-dim)">{{ $item['number'] }}</span>
                    </div>
                    <h3 style="font-size:16.5px;font-weight:600;margin:16px 0 8px">{{ $item['title'] }}</h3>
                    <p style="font-size:13.5px;line-height:1.6;color:var(--lp-muted);margin:0">{{ $item['text'] }}</p>
                </div>
            @endforeach
        </div>
    </section>

    {{-- Download --}}
    <section id="download" class="lp-section">
        <div class="lp-section-head" style="margin-bottom:clamp(28px,4.5vh,44px)">
            <div style="display:flex;flex-wrap:wrap;align-items:center;justify-content:center;gap:10px;margin:0 0 14px">
                <p class="lp-eyebrow" style="margin:0">Multiplataforma</p>
                <span class="lp-version">{{ $versionLabel }}</span>
            </div>
            <h2 data-reveal class="lp-h2">Baixar Unkvoid</h2>
            <p class="lp-lead">Instaladores nativos para macOS, Windows e Linux. O app se atualiza sozinho.</p>
        </div>

        <div data-reveal style="display:flex;flex-direction:column;align-items:center;gap:14px;margin-bottom:clamp(30px,5vh,48px)">
            <a :href="primaryHref" class="lp-btn-primary" style="align-items:baseline;flex-wrap:wrap;justify-content:center;gap:8px 14px;font-size:clamp(17px,2.4vw,22px);letter-spacing:-0.02em;padding:clamp(17px,2.4vw,22px) clamp(24px,3.4vw,40px);border-radius:16px;box-shadow:0 22px 60px -22px rgba(140,80,240,0.9)">
                <span class="lp-os"><span x-show="os === 'Windows'"><x-os.windows /></span><span x-show="os === 'macOS'"><x-os.apple /></span><span x-show="os === 'Linux'"><x-os.linux /></span></span>
                <span x-text="primaryLabel">Baixar</span>
            </a>
            <div class="lp-mono" style="display:flex;flex-wrap:wrap;justify-content:center;gap:6px 16px;font-size:12.5px">
                <template x-for="platform in others" :key="platform">
                    <a :href="links[platform] ?? '#download'" class="lp-link"><span class="lp-os"><span x-show="platform === 'Windows'"><x-os.windows /></span><span x-show="platform === 'macOS'"><x-os.apple /></span><span x-show="platform === 'Linux'"><x-os.linux /></span></span><span x-text="platform"></span></a>
                </template>
            </div>

            <p style="font-size:12.5px;color:var(--lp-dim);margin:0;text-align:center;max-width:52ch">Ao baixar, você concorda com os <a href="{{ route('terms') }}" class="lp-link" wire:navigate>Termos de uso</a> e a <a href="{{ route('privacy') }}" class="lp-link" wire:navigate>Política de privacidade</a>. O app manda o trecho do log quando dá erro, para a gente conseguir consertar.</p>

            <p style="font-size:12.5px;color:var(--lp-dim);margin:0;text-align:center"><a href="{{ route('code-signing') }}" class="lp-link" wire:navigate>Code signing policy</a></p>
        </div>

        <div class="lp-dl-grid">
            <div data-reveal class="lp-dl-card" :class="{ 'is-mine': os === 'macOS' }">
                <span class="lp-mine" x-show="os === 'macOS'" x-cloak>Seu sistema</span>
                <div style="display:flex;align-items:center;justify-content:space-between;gap:10px">
                    <div class="lp-dl-icon"><x-os.apple /></div>
                    <span class="lp-ext">.dmg</span>
                </div>
                <div>
                    <h3 style="font-size:19px;font-weight:600;letter-spacing:-0.02em;margin:0">macOS</h3>
                    <p style="font-size:13px;color:var(--lp-muted);margin:6px 0 0">Apple Silicon. Na primeira abertura, libere em Ajustes do Sistema › Privacidade e Segurança › "Abrir mesmo assim".</p>
                </div>
                <div style="margin-top:auto;display:flex;flex-direction:column;gap:8px">
                    @if (is_null($links['macOS']))
                        <span class="lp-dl-btn is-off">Em breve</span>
                    @else
                        <a href="{{ $links['macOS'] }}" class="lp-dl-btn">Baixar .dmg</a>
                    @endif
                </div>
            </div>

            <div data-reveal class="lp-dl-card" :class="{ 'is-mine': os === 'Windows' }">
                <span class="lp-mine" x-show="os === 'Windows'" x-cloak>Seu sistema</span>
                <div style="display:flex;align-items:center;justify-content:space-between;gap:10px">
                    <div class="lp-dl-icon"><x-os.windows /></div>
                    <span class="lp-ext">.exe / .msi</span>
                </div>
                <div>
                    <h3 style="font-size:19px;font-weight:600;letter-spacing:-0.02em;margin:0">Windows</h3>
                    <p style="font-size:13px;color:var(--lp-muted);margin:6px 0 0">Instalador ou pacote MSI.</p>
                </div>
                <div style="margin-top:auto;display:flex;flex-direction:column;gap:8px">
                    @if (is_null($links['WindowsExe']))
                        <span class="lp-dl-btn is-off">.exe em breve</span>
                    @else
                        <a href="{{ $links['WindowsExe'] }}" class="lp-dl-btn">Baixar .exe</a>
                    @endif

                    @if (is_null($links['WindowsMsi']))
                        <span class="lp-dl-sub" style="color:var(--lp-dim);border-style:dashed">.msi em breve</span>
                    @else
                        <a href="{{ $links['WindowsMsi'] }}" @class(['lp-dl-btn' => is_null($links['WindowsExe']), 'lp-dl-sub' => ! is_null($links['WindowsExe'])])>Baixar .msi</a>
                    @endif
                </div>
            </div>

            <div data-reveal class="lp-dl-card is-wide" :class="{ 'is-mine': os === 'Linux' }">
                <span class="lp-mine" x-show="os === 'Linux'" x-cloak>Seu sistema</span>
                <div class="lp-dl-body">
                    <div style="display:flex;flex-direction:column;gap:16px;height:100%">
                        <div style="display:flex;align-items:center;justify-content:space-between;gap:10px">
                            <div class="lp-dl-icon"><x-os.linux /></div>
                            <span class="lp-ext">.deb / apt</span>
                        </div>
                        <div>
                            <h3 style="font-size:19px;font-weight:600;letter-spacing:-0.02em;margin:0">Linux</h3>
                            <p style="font-size:13px;color:var(--lp-muted);margin:6px 0 0">Debian e Ubuntu. Instala e atualiza pelo apt, junto com o resto do sistema. Por enquanto só assiste.</p>
                        </div>
                        <div style="margin-top:auto">
                            @if (is_null($links['Linux']))
                                <span class="lp-dl-btn is-off">.deb em breve</span>
                            @else
                                <a href="{{ $links['Linux'] }}" class="lp-dl-btn">Baixar o .deb</a>
                                <p style="font-size:11.5px;color:var(--lp-dim);margin:8px 0 0">Instale com <span class="lp-mono">sudo apt install ./arquivo.deb</span>, que puxa as dependências junto. Pelo repositório acima o apt atualiza sozinho.</p>
                            @endif
                        </div>
                    </div>
                    <div class="lp-code">
                        <span class="lp-step">1 · adicione o repositório, uma vez</span>
                        <code>curl -fsSL {{ $aptUrl }}/unkvoid.gpg | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] {{ $aptUrl }} ./" | sudo tee /etc/apt/sources.list.d/unkvoid.list</code>
                        <span class="lp-step">2 · instale</span>
                        <code>sudo apt update && sudo apt install unkvoid</code>
                        <button type="button" class="lp-copy" @click="copyApt()" x-text="copied ? 'Copiado' : 'Copiar tudo'">Copiar tudo</button>
                    </div>
                </div>
            </div>
        </div>
    </section>
</div>
