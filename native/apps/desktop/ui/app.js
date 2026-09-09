import { SfuClient } from './SfuClient.js';

import { Broadcast } from './broadcast.js';
import { isRoomCode, newRoomCode } from './room-code.js';

const el = id => document.getElementById(id);

const GRID_ICON = '<path stroke-linecap="round" stroke-linejoin="round" d="M4 5h6v6H4zM14 5h6v6h-6zM4 13h6v6H4zM14 13h6v6h-6z"/>';
const FOCUS_ICON = '<path stroke-linecap="round" stroke-linejoin="round" d="M4 5h16v10H4zM4 17h4v2H4zM10 17h4v2h-4zM16 17h4v2h-4z"/>';

/** Aparência dos elementos que o JavaScript cria. */
const LOOK = {
    tile: 'group relative m-0 flex min-h-0 flex-col overflow-hidden rounded-lg bg-black',
    tileFocused: 'row-span-full col-span-full',
};

// Esta interface não tem console: um erro de JavaScript aqui vira tela preta sem pista
// nenhuma. Ler a ponte às cegas quebraria exatamente assim, então a falha vai para a
// tela — foi o que uma vez deixou a tela de atualização girando para sempre.
if (! window.__TAURI__?.core) {
    el('update-status').textContent = 'Build quebrada: a ponte do Tauri não carregou.';
    throw new Error('window.__TAURI__ is missing — check withGlobalTauri in tauri.conf.json');
}

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

/**
 * Unkvoid: um nome, uma sala, e a tela de quem quiser mostrar.
 *
 * Não há conta, servidor, canal nem chat — o código da sala é a única chave. Quem cria
 * manda o código para os amigos, e quem tem o código entra. Nada disso é guardado: a
 * sala existe enquanto tiver gente dentro.
 */
class App {
    /** De quanto em quanto tempo procurar versão nova com o app já aberto. */
    static UPDATE_EVERY_MS = 6 * 60 * 60 * 1000;

    /** Mantém o diagnóstico recente pequeno para o modal abrir sem travar o WebView. */
    static MAX_LOG_ENTRIES = 250;
    static MAX_LOG_CHARS = 64 * 1024;

    /** Onde o nome fica entre uma abertura e outra. Ninguém quer redigitar todo dia. */
    static NAME_KEY = 'unkvoid:name';

    /** O último código fica como lembrete, mas não entra automaticamente na sala. */
    static ROOM_KEY = 'unkvoid:last-room';

    /**
     * O único servidor. VITE_SERVER aponta um build local para uma pilha local, e o
     * override no localStorage serve para cutucar um build já pronto sem recompilar.
     */
    static SERVER = import.meta.env.VITE_SERVER ?? localStorage.getItem('server') ?? 'https://discord.unkvoid.com';

    /** A sinalização mora no mesmo host, atrás do mesmo TLS. */
    static socketUrl() {
        return `${App.SERVER.replace(/^http/, 'ws')}/sfu`;
    }

    constructor() {
        this.logs = [];
        this.logChars = 0;
        this.log('app.start', { userAgent: navigator.userAgent, platform: navigator.platform });
        this.name = '';
        this.room = null;
        this.sfu = null;
        this.broadcast = null;
        this.sharing = false;
        this.shareSource = null;
        this.shareSources = null;
        this.previewTimers = new Set();
        this.previewInFlight = new Set();
        this.broadcastStatsTimer = null;
        this.lastBroadcastStats = null;
        this.broadcastStatsAt = 0;
        this.mediaStatsTimers = new Map();
        this.remoteAudios = new Map();
        this.consumingProducers = new Set();

        /** Grade mostra todos do mesmo tamanho; foco dá a tela toda a um só. */
        this.focused = null;

        this.attempt = 0;
        this.reconnect = null;
    }

    /**
     * Ordem de abertura: atualizar, exigir o servidor, e só então deixar entrar. Entrar
     * num app desatualizado ou sem servidor só produziria erro mais adiante.
     */
    async start() {
        this.showDownloadProgress();

        await this.serverAnswered();

        el('update-screen').hidden = true;

        setInterval(() => void this.update(), App.UPDATE_EVERY_MS);

        if (! await this.serverAnswered()) {
            return;
        }

        this.showEntry();
    }

    /**
     * Quanto já baixou, na tela.
     *
     * Sem isto ela fica parada em "Procurando atualizações…" durante todo o download, e
     * a leitura correta de uma tela parada é "travou".
     */
    showDownloadProgress() {
        void listen('update:progress', ({ payload: [baixado, total] }) => {
            el('update-status').textContent = total
                ? `Baixando a atualização… ${Math.round((baixado / total) * 100)}%`
                : `Baixando a atualização… ${(baixado / 1024 / 1024).toFixed(1)} MB`;
        });
    }

    /**
     * Procura, baixa e instala a versão nova — sem perguntar nada.
     *
     * Roda na abertura e de tempos em tempos: o app inicia com o sistema e fica dias
     * aberto na bandeja, então só olhar na abertura significaria esperar o próximo
     * reinício da máquina para ver uma versão publicada hoje.
     */
    async update() {
        try {
            const version = await invoke('check_update');

            if (! version) {
                return;
            }

            el('update-status').textContent = `Instalando a versão ${version}…`;

            // Reiniciar no meio de uma transmissão derruba quem está assistindo. A
            // versão já está no disco; passa a valer no próximo reinício.
            if (this.room) {
                return;
            }

            await invoke('restart');
        } catch (failure) {
            // Falhar a atualização não pode impedir o app de abrir: ele segue na versão
            // atual, que funciona.
            console.warn('atualização indisponível:', failure);
        }
    }

    async serverAnswered() {
        try {
            const response = await fetch(`${App.SERVER}/health`);

            if (! response.ok) {
                throw new Error(`o servidor respondeu ${response.status}`);
            }

            const health = await response.json();
            const localVersion = await invoke('app_version');

            if (health.appVersion && health.appVersion !== localVersion) {
                await this.update();
            }

            return true;
        } catch {
            this.showOffline();

            return false;
        }
    }

    showOffline() {
        el('offline-screen').hidden = false;

        this.attempt += 1;
        el('offline-status').textContent = `Reconectando… (tentativa ${this.attempt})`;

        clearTimeout(this.reconnect);
        this.reconnect = setTimeout(async () => {
            if (await this.serverAnswered()) {
                el('offline-screen').hidden = true;
                this.attempt = 0;
                this.showEntry();
            }
        }, Math.min(2000 * this.attempt, 10000));
    }

    showEntry() {
        el('entry-screen').hidden = false;
        el('room').hidden = true;

        el('my-name').value = localStorage.getItem(App.NAME_KEY) ?? '';
        el('room-code').value = localStorage.getItem(App.ROOM_KEY) ?? '';
        el('my-name').focus();

        el('create-room').onclick = () => {
            const nome = el('room-code').value.trim().toLowerCase();
            void this.enterRoom(nome || newRoomCode());
        };
        el('join-form').onsubmit = event => {
            event.preventDefault();
            void this.enterRoom(el('room-code').value.trim().toLowerCase());
        };
    }

    /** Criar sorteia um código novo; entrar usa o que a pessoa colou. */
    async enterRoom(code) {
        const name = el('my-name').value.trim();

        el('entry-error').textContent = '';

        if (! name) {
            el('entry-error').textContent = 'Escolha um nome primeiro.';
            el('my-name').focus();

            return;
        }

        if (! isRoomCode(code)) {
            el('entry-error').textContent = 'Use 3–32 caracteres: letras, números e hífens (sem hífen no começo ou fim).';
            el('room-code').focus();

            return;
        }

        localStorage.setItem(App.NAME_KEY, name);
        localStorage.setItem(App.ROOM_KEY, code);

        this.name = name;
        this.room = code;

        await this.connect();
    }

    async connect() {
        el('entry-screen').hidden = true;
        el('room').hidden = false;
        el('copy-code').textContent = this.room;
        el('empty-code').textContent = this.room;
        el('room-people').textContent = 'conectando…';

        this.paintLayout();
        this.wireRoom();

        try {
            this.sfu = new SfuClient();
            this.sfu.addEventListener('diagnostic', event => this.log(event.detail.event, event.detail.data));
            this.sfu.addEventListener('newProducer', event => this.consume(event.detail));
            this.sfu.addEventListener('peersChanged', () => this.refreshPeople());
            this.sfu.addEventListener('peerLeft', event => this.showScreen(event.detail.peerId, null));
            this.sfu.addEventListener('producerClosed', event => {
                if (event.detail.kind === 'video' && event.detail.source === 'screen') {
                    this.showScreen(event.detail.peerId, null);
                }
            });
            this.sfu.addEventListener('consumerClosed', event => {
                if (event.detail.kind === 'video') {
                    this.showScreen(event.detail.peerId, null);
                }
            });

            this.broadcast = new Broadcast(this.sfu);

            const joined = await this.sfu.connect(App.socketUrl(), { room: this.room, name: this.name });

            // Quem já estava transmitindo antes de você chegar não emite `newProducer`:
            // sem varrer a lista inicial, você entra numa sala com telas ao vivo e não vê
            // nenhuma até alguém recomeçar.
            for (const peer of joined.peers) {
                for (const producer of peer.producers) {
                    await this.consume({ ...producer, peerId: peer.peerId });
                }
            }

            this.refreshPeople();
        } catch (failure) {
            this.fail(`não deu para entrar na sala: ${failure.message}`);
            await this.leave();
        }
    }

    wireRoom() {
        el('copy-code').onclick = () => this.copyCode();
        el('room-people').onclick = () => {
            const lista = el('people-list');
            lista.hidden = ! lista.hidden;
            if (! lista.hidden) {
                this.drawPeopleList();
            }
        };
        el('layout').onclick = () => this.toggleLayout();
        el('share').onclick = () => this.openShareModal();
        el('stop').onclick = () => this.stopSharing();
        el('leave').onclick = () => this.leave();
        el('logs').onclick = () => this.openLogs();
        el('logs-close').onclick = () => { el('logs-modal').hidden = true; };
        el('logs-clear').onclick = () => {
            this.logs = [];
            this.logChars = 0;
            this.renderLogs();
        };
        el('logs-copy').onclick = () => this.copyLogs();
        el('share-cancel').onclick = () => this.closeShareModal();
        el('share-confirm').onclick = async () => {
            this.closeShareModal();
            await this.share();
        };
    }

    /** O código só serve se chegar ao amigo, então copiar é um clique e um aviso. */
    async copyCode() {
        try {
            await navigator.clipboard.writeText(this.room);
            el('copy-code').textContent = 'copiado!';
            setTimeout(() => { el('copy-code').textContent = this.room; }, 1200);
        } catch {
            this.fail('não deu para copiar — selecione o código à mão.');
        }
    }

    fail(mensagem) {
        this.log('ui.error', { message: mensagem });
        el('room-error').textContent = mensagem;
        el('room-error').hidden = false;
    }

    refreshPeople() {
        const total = [...(this.sfu?.peers?.values() ?? [])]
            .filter(peer => ! peer.reconnecting)
            .length || 1;

        el('room-people').textContent = total === 1 ? '1 pessoa' : `${total} pessoas`;

        if (! el('people-list').hidden) {
            this.drawPeopleList();
        }
    }

    drawPeopleList() {
        const lista = el('people-list-items');
        lista.innerHTML = '';

        const pessoas = [...(this.sfu?.peers?.values() ?? [])]
            .filter(peer => ! peer.reconnecting);

        for (const pessoa of pessoas) {
            const item = document.createElement('div');

            item.className = 'flex items-center gap-2 rounded px-2 py-1.5 text-sm text-white';
            item.innerHTML = '<span class="size-2 shrink-0 rounded-full bg-emerald-400"></span>'
                + '<span class="min-w-0 flex-1 truncate"></span>'
                + '<span class="text-xs text-ink-soft"></span>';
            item.querySelectorAll('span')[1].textContent = pessoa.name;
            item.querySelectorAll('span')[2].textContent = pessoa.sharing ? 'compartilhando' : '';
            lista.appendChild(item);
        }

        if (! pessoas.length) {
            lista.innerHTML = '<p class="text-sm text-ink-soft">Nenhuma pessoa conectada.</p>';
        }
    }

    async consume({ producerId, peerId: ownerPeerId }) {
        if (this.consumingProducers.has(producerId) || this.sfu?.consumersHasProducer?.(producerId)) {
            return;
        }

        this.consumingProducers.add(producerId);
        this.log('media.consume.start', { producerId });
        try {
            const { consumer, peerId } = await this.sfu.consume(producerId);

            if (consumer.kind === 'audio') {
                // O áudio da tela transmitida: o som do jogo, do vídeo. Não há microfone
                // neste app, então este é o único som que existe.
                const audio = document.createElement('audio');

                audio.srcObject = new MediaStream([consumer.track]);
                audio.autoplay = true;
                audio.volume = 1;
                audio.onplay = () => this.log('media.audio.playing', { peerId });
                audio.onerror = () => this.log('media.audio.error', {
                    peerId,
                    message: audio.error?.message ?? `media error ${audio.error?.code ?? 'unknown'}`,
                });
                audio.dataset.remote = peerId;
                document.body.appendChild(audio);
                this.remoteAudios.set(peerId, audio);
                const quadro = document.querySelector(`[data-screen="${peerId}"]`);
                if (quadro) {
                    this.attachAudioControl(peerId, quadro);
                }
                void audio.play().catch(error => this.log('media.audio.autoplay.error', {
                    peerId,
                    message: error.message ?? String(error),
                }));

                return;
            }

            this.showScreen(peerId, new MediaStream([consumer.track]));
            this.log('media.consume.ready', { producerId, peerId, kind: consumer.kind });
        } catch (failure) {
            this.log('media.consume.error', { producerId, peerId: ownerPeerId, message: failure.message ?? String(failure) });
            console.warn('não deu para receber a mídia:', failure);
        } finally {
            this.consumingProducers.delete(producerId);
        }
    }

    /** Desenha (ou remove) a tela de quem está transmitindo. */
    showScreen(from, stream) {
        const existente = document.querySelector(`[data-screen="${from}"]`);

        if (! stream) {
            existente?.remove();
            document.querySelector(`audio[data-remote="${from}"]`)?.remove();
            this.remoteAudios.delete(from);

            if (this.focused === from) {
                this.focused = null;
            }

            clearInterval(this.mediaStatsTimers.get(from));
            this.mediaStatsTimers.delete(from);
            this.paintLayout();

            return;
        }

        const quadro = existente ?? document.createElement('figure');

        quadro.dataset.screen = from;
        quadro.innerHTML = '<video class="min-h-0 w-full flex-1 bg-black object-contain" autoplay playsinline></video>'
            + '<figcaption class="flex items-center gap-2 bg-panel px-3 py-1.5 text-xs text-ink">'
            + '<span class="truncate"></span>'
            + '<span class="text-ink-dim" data-media-stats>buffer -- · fps --</span>'
            + '<span class="flex items-center gap-1.5 text-ink-soft" data-audio-control hidden>'
            + '<span aria-hidden="true">🔊</span>'
            + '<input class="w-20 accent-brand" data-audio-volume type="range" min="0" max="100" value="100" aria-label="Volume desta transmissão">'
            + '<span data-audio-volume-value>100%</span>'
            + '</span>'
            + '<span class="flex-1"></span>'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-focus type="button">Focar</button>'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-fullscreen type="button">Tela cheia</button>'
            + '</figcaption>';

        const video = quadro.querySelector('video');
        video.srcObject = stream;
        this.attachAudioControl(from, quadro);
        video.onerror = () => this.log('media.video.error', {
            peerId: from,
            message: video.error?.message ?? `media error ${video.error?.code ?? 'unknown'}`,
        });
        video.onstalled = () => this.log('media.video.stalled', { peerId: from });
        video.onwaiting = () => this.log('media.video.waiting', { peerId: from });
        video.onended = () => this.log('media.video.ended', { peerId: from });
        quadro.querySelector('span').textContent = this.sfu?.peers?.get(from)?.name ?? 'transmitindo';
        quadro.querySelector('[data-focus]').onclick = () => this.focus(from);
        quadro.querySelector('[data-fullscreen]').onclick = async () => {
            try {
                if (document.fullscreenElement) {
                    await document.exitFullscreen();
                    return;
                }

                await quadro.requestFullscreen?.();
            } catch (error) {
                this.log('media.fullscreen.error', { peerId: from, message: error.message ?? String(error) });
            }
        };

        if (! existente) {
            el('stage').appendChild(quadro);
        }

        this.startMediaStats(from, video);
        this.paintLayout();
    }

    attachAudioControl(peerId, quadro) {
        const audio = this.remoteAudios.get(peerId);
        const control = quadro.querySelector('[data-audio-control]');

        if (! audio || ! control || control.dataset.ready === 'true') {
            if (audio && control) {
                control.hidden = false;
            }

            return;
        }

        const input = control.querySelector('[data-audio-volume]');
        const value = control.querySelector('[data-audio-volume-value]');
        const volume = Math.round(audio.volume * 100);

        input.value = String(volume);
        value.textContent = `${volume}%`;
        input.oninput = event => {
            const next = Number(event.target.value);
            audio.volume = next / 100;
            value.textContent = `${next}%`;
            this.log('media.audio.volume', { peerId, volume: next / 100 });
        };
        control.dataset.ready = 'true';
        control.hidden = false;
    }

    startMediaStats(peerId, video) {
        clearInterval(this.mediaStatsTimers.get(peerId));

        let frames = 0;
        let lastFrames = 0;
        let lastSample = performance.now();
        const atualizar = () => {
            const quadro = document.querySelector(`[data-screen="${peerId}"]`);
            const stats = quadro?.querySelector('[data-media-stats]');

            if (! quadro || ! stats) {
                clearInterval(this.mediaStatsTimers.get(peerId));
                this.mediaStatsTimers.delete(peerId);

                return;
            }

            const agora = performance.now();
            const decorrido = Math.max(agora - lastSample, 1);
            const buffer = video.buffered.length
                ? Math.max(0, video.buffered.end(video.buffered.length - 1) - video.currentTime)
                : 0;
            const fps = Math.round((frames - lastFrames) * 1000 / decorrido);
            const quality = video.getVideoPlaybackQuality?.();

            stats.textContent = `buffer ${buffer.toFixed(1)} s · fps ${fps}`;
            this.log('media.stats', {
                peerId,
                pingMs: this.sfu?.lastRttMs ?? null,
                bufferSeconds: Number(buffer.toFixed(2)),
                fps,
                framesDropped: quality?.droppedVideoFrames ?? null,
                framesDecoded: quality?.totalVideoFrames ?? null,
            });
            lastFrames = frames;
            lastSample = agora;
        };

        const contarFrame = () => {
            frames += 1;
            if ('requestVideoFrameCallback' in video) {
                video.requestVideoFrameCallback(contarFrame);
            }
        };

        if ('requestVideoFrameCallback' in video) {
            video.requestVideoFrameCallback(contarFrame);
        } else {
            const contar = () => { frames += 1; };
            video.addEventListener('timeupdate', contar);
        }

        const timer = setInterval(atualizar, 1000);
        this.mediaStatsTimers.set(peerId, timer);
        atualizar();
    }

    /** Uma tela ocupando tudo, ou de volta para a grade. */
    focus(from) {
        this.focused = this.focused === from ? null : from;
        this.paintLayout();
    }

    toggleLayout() {
        const primeira = el('stage').firstElementChild?.dataset.screen ?? null;

        this.focused = this.focused ? null : primeira;
        this.paintLayout();
    }

    /**
     * Grade ou foco, e o palco só aparece quando há o que mostrar.
     *
     * As colunas saem da raiz do total: 1 tela ocupa tudo, 2 a 4 ficam em 2 colunas, 5 a
     * 9 em 3. Fixar em 2 colunas deixava cinco telas em fileiras finas e ilegíveis.
     */
    paintLayout() {
        const quadros = [...el('stage').children];

        el('stage').hidden = ! quadros.length;
        el('stage-empty').hidden = Boolean(quadros.length);
        el('layout-icon').innerHTML = this.focused ? FOCUS_ICON : GRID_ICON;

        const colunas = Math.ceil(Math.sqrt(quadros.length || 1));

        el('stage').style.gridTemplateColumns = `repeat(${this.focused ? 1 : colunas}, minmax(0, 1fr))`;

        for (const quadro of quadros) {
            const escondido = Boolean(this.focused) && quadro.dataset.screen !== this.focused;

            quadro.className = LOOK.tile;
            quadro.hidden = escondido;
        }
    }

    /**
     * Abre a escolha do que transmitir.
     *
     * A lista vem do sistema operacional, não de um palpite: são os mesmos dados que o
     * macOS usa para montar o seletor dele.
     */
    async openShareModal() {
        this.shareSource = null;
        el('share-confirm').disabled = true;
        el('share-modal').hidden = false;

        for (const aba of document.querySelectorAll('[data-tab]')) {
            aba.onclick = () => this.drawShareTab(aba.dataset.tab);
        }

        const [telas, janelas] = await Promise.all([
            invoke('list_displays').catch(() => []),
            invoke('list_windows').catch(() => []),
        ]);

        this.shareSources = {
            display: telas.map(tela => ({
                value: `display:${tela.id}`,
                label: `Tela ${tela.id}`,
                detail: `${tela.width}×${tela.height}`,
            })),
            // Janela sem título é painel de sistema: mostrar só polui a escolha.
            window: janelas
                .filter(janela => janela.title.trim())
                .map(janela => ({
                    value: `window:${janela.id}`,
                    label: janela.title,
                    detail: janela.application,
                })),
        };

        this.drawShareTab('display');
    }

    /**
     * Uma aba por vez, com miniatura de cada item.
     *
     * O nome sozinho não basta: duas janelas chamadas "Terminal" são indistinguíveis, e
     * escolher errado manda para a sala o que a pessoa não queria mostrar.
     */
    drawShareTab(tab) {
        const lista = el('share-sources');
        const itens = this.shareSources?.[tab] ?? [];

        for (const aba of document.querySelectorAll('[data-tab]')) {
            const ativa = aba.dataset.tab === tab;

            aba.classList.toggle('border-brand', ativa);
            aba.classList.toggle('text-white', ativa);
            aba.classList.toggle('border-transparent', ! ativa);
            aba.classList.toggle('text-ink-soft', ! ativa);
        }

        this.shareSource = null;
        el('share-confirm').disabled = true;
        lista.innerHTML = '';

        if (! itens.length) {
            lista.innerHTML = tab === 'display'
                ? '<p class="text-sm text-ink-soft">Nenhuma tela encontrada. No macOS, autorize a gravação de tela nas Configurações do Sistema.</p>'
                : '<p class="text-sm text-ink-soft">Nenhuma janela aberta para compartilhar.</p>';

            return;
        }

        lista.className = 'mt-4 grid min-h-0 flex-1 auto-rows-min grid-cols-2 content-start gap-3 overflow-y-auto';

        for (const item of itens) {
            const botao = document.createElement('button');

            botao.type = 'button';
            botao.dataset.source = item.value;
            botao.className = 'cursor-pointer overflow-hidden rounded-lg border-2 border-transparent bg-rail text-left transition-colors hover:border-brand';
            botao.innerHTML = '<div class="flex aspect-video items-center justify-center bg-black">'
                + '<img class="size-full object-contain" alt="" hidden>'
                + '<span class="text-xs text-ink-dim">sem prévia</span>'
                + '</div>'
                + '<div class="px-2.5 py-2">'
                + '<p class="truncate text-sm text-white"></p>'
                + '<p class="truncate text-xs text-ink-soft"></p>'
                + '</div>';

            const [nome, detalhe] = botao.querySelectorAll('p');

            nome.textContent = item.label;
            detalhe.textContent = item.detail ?? '';
            botao.onclick = () => this.pickShareSource(botao);
            lista.appendChild(botao);

            // Uma miniatura por vez, sem travar a abertura do seletor: quem tem dez
            // janelas abertas veria a lista congelar esperando todas.
            const atualizar = async () => {
                if (this.previewInFlight.has(item.value)) {
                    return;
                }

                this.previewInFlight.add(item.value);

                try {
                    const dados = await invoke('source_preview', { source: item.value });

                    if (! dados || ! botao.isConnected) {
                        return;
                    }

                    const imagem = botao.querySelector('img');

                    imagem.src = dados;
                    imagem.hidden = false;
                    botao.querySelector('span').hidden = true;
                } catch {
                    // A janela pode desaparecer enquanto o seletor está aberto.
                } finally {
                    this.previewInFlight.delete(item.value);
                }
            };

            void atualizar();
            // Cada preview cria uma captura nativa de um quadro. Não podemos iniciar 30
            // capturas simultâneas por segundo: no Windows isso trava o app inteiro.
            this.previewTimers.add(setInterval(() => void atualizar(), 1000));
        }
    }

    /**
     * Marca o escolhido pela borda, não pelo fundo: o card é quase todo miniatura, e
     * pintar o fundo não aparece atrás da imagem.
     */
    pickShareSource(botao) {
        for (const outro of el('share-sources').querySelectorAll('button')) {
            const escolhido = outro === botao;

            outro.classList.toggle('border-brand', escolhido);
            outro.classList.toggle('border-transparent', ! escolhido);
        }

        this.shareSource = botao.dataset.source;
        el('share-confirm').disabled = false;
    }

    closeShareModal() {
        el('share-modal').hidden = true;
        for (const timer of this.previewTimers) {
            clearInterval(timer);
        }
        this.previewTimers.clear();
        this.previewInFlight.clear();
    }

    async share() {
        this.log('broadcast.start', { quality: el('quality').value, fps: el('fps').value, source: this.shareSource });
        try {
            await this.broadcast.start(el('quality').value, Number(el('fps').value), this.shareSource);
            this.broadcastStatsTimer = setInterval(() => {
                void invoke('broadcast_stats')
                    .then(stats => this.updateBroadcastStats(stats))
                    .catch(error => this.log('broadcast.stats.error', { message: error.message ?? String(error) }));
            }, 1000);
            void invoke('broadcast_stats').then(stats => this.updateBroadcastStats(stats));
            this.paintSharing(true);
        } catch (failure) {
            this.log('broadcast.start.error', { message: failure.message ?? String(failure) });
            // Falhar calado deixava a barra sem botão nenhum: quem tentou compartilhar
            // via o modal fechar e mais nada.
            this.paintSharing(false);
            this.fail(`não deu para transmitir: ${failure.message ?? failure}`);
        }
    }

    paintSharing(on) {
        this.sharing = on;
        el('share').hidden = on;
        el('stop').hidden = ! on;
        if (! on) {
            clearInterval(this.broadcastStatsTimer);
            this.broadcastStatsTimer = null;
            this.lastBroadcastStats = null;
            this.broadcastStatsAt = 0;
            document.querySelector('[data-broadcast-stats]').textContent = 'ping --';
        }
    }

    updateBroadcastStats(stats) {
        if (! stats?.active) {
            return;
        }

        const now = performance.now();
        const previous = this.lastBroadcastStats;
        const elapsed = this.broadcastStatsAt ? Math.max(now - this.broadcastStatsAt, 1) : 1000;
        const fps = previous
            ? Math.round((stats.captured - previous.captured) * 1000 / elapsed)
            : '--';
        const ping = this.sfu?.lastRttMs ?? '--';
        document.querySelector('[data-broadcast-stats]').textContent = `ping ${ping} ms`;
        this.log('broadcast.stats', { ...stats, pingMs: ping, fps });

        if (previous) {
            const errors = {
                encodeErrors: stats.encodeErrors - previous.encodeErrors,
                sendErrors: stats.sendErrors - previous.sendErrors,
                audioErrors: stats.audioErrors - previous.audioErrors,
            };
            if (Object.values(errors).some(value => value > 0)) {
                this.log('broadcast.error', {
                    reason: 'media pipeline error',
                    ...errors,
                    totals: stats,
                });
            }
        }

        this.lastBroadcastStats = stats;
        this.broadcastStatsAt = now;
    }

    async stopSharing() {
        this.log('broadcast.stop');
        clearInterval(this.broadcastStatsTimer);
        this.broadcastStatsTimer = null;
        await this.broadcast?.stop().catch(() => 0);
        this.paintSharing(false);
    }

    async leave() {
        this.log('room.leave');
        await this.stopSharing();

        // `leaveRoom` antes de `disconnect`: fechar o socket sem avisar deixa você como
        // fantasma na sala até o servidor desistir sozinho.
        await this.sfu?.leaveRoom();
        this.sfu?.disconnect();

        this.sfu = null;
        this.broadcast = null;
        this.room = null;
        this.remoteAudios.clear();
        this.focused = null;
        el('people-list').hidden = true;

        el('stage').innerHTML = '';
        el('room-error').hidden = true;
        document.querySelectorAll('audio[data-remote]').forEach(audio => audio.remove());

        this.showEntry();
    }

    log(event, data = {}) {
        const line = `${new Date().toISOString()} ${event} ${JSON.stringify(data)}`;
        this.logs.push(line);
        this.logChars += line.length + (this.logs.length > 1 ? 1 : 0);

        while (this.logs.length > App.MAX_LOG_ENTRIES || this.logChars > App.MAX_LOG_CHARS) {
            const removed = this.logs.shift();
            this.logChars -= removed.length + 1;
        }
    }

    renderLogs() {
        el('logs-output').value = this.logs.join('\n');
        el('logs-output').scrollTop = el('logs-output').scrollHeight;
    }

    openLogs() {
        el('logs-modal').hidden = false;
        this.renderLogs();
    }

    async copyLogs() {
        this.renderLogs();
        await navigator.clipboard.writeText(el('logs-output').value);
        el('logs-copy').textContent = 'Copiado!';
        setTimeout(() => { el('logs-copy').textContent = 'Copiar logs'; }, 1200);
    }
}

const app = new App();

void app.start();

// Exposto de propósito: a janela do Tauri não tem console, e é por aqui que dá para
// cutucar o estado do app pelo harness.
window.unkvoid = app;
