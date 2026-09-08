import { SfuClient } from '@voice/SfuClient.js';

import { Api } from './api.js';
import { Broadcast } from './broadcast.js';

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

    /** Onde o nome fica entre uma abertura e outra. Ninguém quer redigitar todo dia. */
    static NAME_KEY = 'unkvoid:name';

    constructor() {
        this.api = new Api(() => this.showOffline());

        this.room = null;
        this.sfu = null;
        this.broadcast = null;
        this.sharing = false;
        this.shareSource = null;
        this.shareSources = null;

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

        await this.update();

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
            const response = await fetch(`${Api.BASE}/api/health`);

            if (! response.ok) {
                throw new Error(`o servidor respondeu ${response.status}`);
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
        el('my-name').focus();

        el('create-room').onclick = () => this.enterRoom(null);
        el('join-form').onsubmit = event => {
            event.preventDefault();
            void this.enterRoom(el('room-code').value.trim().toLowerCase());
        };
    }

    /** Sem código, o servidor sorteia uma sala nova. Com código, entra na de alguém. */
    async enterRoom(code) {
        const name = el('my-name').value.trim();

        el('entry-error').textContent = '';

        if (! name) {
            el('entry-error').textContent = 'Escolha um nome primeiro.';
            el('my-name').focus();

            return;
        }

        localStorage.setItem(App.NAME_KEY, name);

        try {
            this.room = await this.api.room(name, code || null);
        } catch (failure) {
            el('entry-error').textContent = failure.message;

            return;
        }

        await this.connect();
    }

    async connect() {
        el('entry-screen').hidden = true;
        el('room').hidden = false;
        el('copy-code').textContent = this.room.room;
        el('empty-code').textContent = this.room.room;
        el('room-people').textContent = 'conectando…';

        this.paintLayout();
        this.wireRoom();

        try {
            this.sfu = new SfuClient();
            this.sfu.addEventListener('newProducer', event => this.consume(event.detail));
            this.sfu.addEventListener('peersChanged', () => this.refreshPeople());
            this.sfu.addEventListener('peerLeft', event => this.showScreen(event.detail.peerId, null));

            this.broadcast = new Broadcast(this.sfu);

            const joined = await this.sfu.connect(
                this.room.url,
                async () => (await this.api.room(el('my-name').value.trim(), this.room.room)).token,
            );

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
        el('layout').onclick = () => this.toggleLayout();
        el('share').onclick = () => this.openShareModal();
        el('stop').onclick = () => this.stopSharing();
        el('leave').onclick = () => this.leave();
        el('share-cancel').onclick = () => this.closeShareModal();
        el('share-confirm').onclick = async () => {
            this.closeShareModal();
            await this.share();
        };
    }

    /** O código só serve se chegar ao amigo, então copiar é um clique e um aviso. */
    async copyCode() {
        try {
            await navigator.clipboard.writeText(this.room.room);
            el('copy-code').textContent = 'copiado!';
            setTimeout(() => { el('copy-code').textContent = this.room.room; }, 1200);
        } catch {
            this.fail('não deu para copiar — selecione o código à mão.');
        }
    }

    fail(mensagem) {
        el('room-error').textContent = mensagem;
        el('room-error').hidden = false;
    }

    refreshPeople() {
        const total = this.sfu?.peers?.size ?? 1;

        el('room-people').textContent = total > 1 ? `${total} pessoas` : 'só você por aqui';
    }

    async consume({ producerId }) {
        try {
            const { consumer, peerId } = await this.sfu.consume(producerId);

            if (consumer.kind === 'audio') {
                // O áudio da tela transmitida: o som do jogo, do vídeo. Não há microfone
                // neste app, então este é o único som que existe.
                const audio = document.createElement('audio');

                audio.srcObject = new MediaStream([consumer.track]);
                audio.autoplay = true;
                audio.dataset.remote = peerId;
                document.body.appendChild(audio);

                return;
            }

            this.showScreen(peerId, new MediaStream([consumer.track]));
        } catch (failure) {
            console.warn('não deu para receber a mídia:', failure);
        }
    }

    /** Desenha (ou remove) a tela de quem está transmitindo. */
    showScreen(from, stream) {
        const existente = document.querySelector(`[data-screen="${from}"]`);

        if (! stream) {
            existente?.remove();
            document.querySelector(`audio[data-remote="${from}"]`)?.remove();

            if (this.focused === from) {
                this.focused = null;
            }

            this.paintLayout();

            return;
        }

        const quadro = existente ?? document.createElement('figure');

        quadro.dataset.screen = from;
        quadro.innerHTML = '<video class="min-h-0 w-full flex-1 bg-black object-contain" autoplay playsinline></video>'
            + '<figcaption class="flex items-center gap-2 bg-panel px-3 py-1.5 text-xs text-ink">'
            + '<span class="truncate"></span>'
            + '<span class="flex-1"></span>'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-focus type="button">Focar</button>'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-fullscreen type="button">Tela cheia</button>'
            + '</figcaption>';

        quadro.querySelector('video').srcObject = stream;
        quadro.querySelector('span').textContent = this.sfu?.peers?.get(from)?.name ?? 'transmitindo';
        quadro.querySelector('[data-focus]').onclick = () => this.focus(from);
        quadro.querySelector('[data-fullscreen]').onclick = () => quadro.requestFullscreen?.();

        if (! existente) {
            el('stage').appendChild(quadro);
        }

        this.paintLayout();
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
            void invoke('source_preview', { source: item.value })
                .then(dados => {
                    if (! dados) {
                        return;
                    }

                    const imagem = botao.querySelector('img');

                    imagem.src = dados;
                    imagem.hidden = false;
                    botao.querySelector('span').hidden = true;
                })
                .catch(() => {});
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
    }

    async share() {
        try {
            await this.broadcast.start(el('quality').value, this.shareSource);
            this.paintSharing(true);
        } catch (failure) {
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
    }

    async stopSharing() {
        await this.broadcast?.stop().catch(() => 0);
        this.paintSharing(false);
    }

    async leave() {
        await this.stopSharing();

        // `leaveRoom` antes de `disconnect`: fechar o socket sem avisar deixa você como
        // fantasma na sala até o servidor desistir sozinho.
        await this.sfu?.leaveRoom();
        this.sfu?.disconnect();

        this.sfu = null;
        this.broadcast = null;
        this.room = null;
        this.focused = null;

        el('stage').innerHTML = '';
        el('room-error').hidden = true;
        document.querySelectorAll('audio[data-remote]').forEach(audio => audio.remove());

        this.showEntry();
    }
}

const app = new App();

void app.start();

// Exposto de propósito: a janela do Tauri não tem console, e é por aqui que dá para
// cutucar o estado do app pelo harness.
window.unkvoid = app;
