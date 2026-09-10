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
    static MAX_WINDOW_SOURCES = 12;
    static MAX_WINDOW_PREVIEWS = 4;

    /** Onde o nome fica entre uma abertura e outra. Ninguém quer redigitar todo dia. */
    static NAME_KEY = 'unkvoid:name';

    /** O último código fica como lembrete, mas não entra automaticamente na sala. */
    static ROOM_KEY = 'unkvoid:last-room';

    /** Marca que a recarga em busca do WebRTC já foi tentada nesta abertura. */
    static WEBRTC_RELOAD_KEY = 'unkvoid:webrtc-reload';

    /**
     * O que rodar quando falta uma peça do sistema.
     *
     * Só o Linux tem remédio por linha de comando: no macOS e no Windows o motor da
     * janela vem com o sistema ou com o instalador, então não há pacote a instalar —
     * o que resta ali é reinstalar o app, e é isso que a mensagem diz.
     */
    static REMEDIO = {
        webrtc: 'sudo apt install -y gstreamer1.0-plugins-good gstreamer1.0-plugins-bad'
            + ' gstreamer1.0-libav gstreamer1.0-nice',
        h264: 'sudo apt install -y gstreamer1.0-libav gstreamer1.0-plugins-ugly',
    };

    static isLinux() {
        return /Linux/i.test(navigator.platform) || /Linux/i.test(navigator.userAgent);
    }

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
        // As duas perguntas que a janela do Linux não responde sozinha: o WebKitGTK
        // entrega WebRTC desligado, e o H.264 só aparece se o GStreamer da distro tiver
        // os plugins. Sem isto, os dois casos dão tela preta sem uma linha de pista.
        this.log('app.start', {
            userAgent: navigator.userAgent,
            platform: navigator.platform,
            hasWebRTC: typeof RTCPeerConnection !== 'undefined',
        });
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
        this.broadcastLine = null;
        this.mediaStatsTimers = new Map();
        this.remoteAudios = new Map();
        this.consumingProducers = new Set();
        this.peopleStatsTimer = null;

        /** Quem esta pausado nao gasta banda nem decoder: o servidor para de mandar. */
        this.pausedPeers = new Set();

        /** A propria tela, guardada para o botao poder mostrar e esconder sem reconsumir. */
        this.selfStream = null;

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
        if (! this.hasWebRTC()) {
            return;
        }

        // Fora do `wireRoom`: aquele roda a cada entrada em sala, e `addEventListener`
        // soma em vez de substituir, ao contrário dos `onclick` do resto do arquivo.
        document.addEventListener('click', event => {
            const list = el('people-list');
            const target = event.target;

            if (! list.hidden && target instanceof Node
                && ! list.contains(target) && target !== el('room-people')) {
                list.hidden = true;
            }
        });

        this.showDownloadProgress();

        await this.serverAnswered();

        el('update-screen').hidden = true;
        await this.expandWindow();

        setInterval(() => void this.update(), App.UPDATE_EVERY_MS);

        if (! await this.serverAnswered()) {
            return;
        }

        this.showEntry();
    }

    /**
     * Sem `RTCPeerConnection` não há o que fazer, e é preciso dizer isso na cara.
     *
     * No Linux a janela é WebKitGTK, que entrega o WebRTC desligado. O Rust liga a
     * configuração na abertura, mas a página que nasceu antes disso continua sem o
     * global — a configuração vale para a próxima. Daí a recarga única.
     *
     * Se depois dela ainda faltar, não é ordem de eventos: é o WebKit da distro
     * compilado sem WebRTC. Aí entrar numa sala só produziria "device not supported"
     * lá na frente, e a pessoa voltaria para a tela de nome sem entender nada — que foi
     * exatamente o que aconteceu.
     */
    hasWebRTC() {
        if (typeof RTCPeerConnection !== 'undefined') {
            return true;
        }

        if (! sessionStorage.getItem(App.WEBRTC_RELOAD_KEY)) {
            sessionStorage.setItem(App.WEBRTC_RELOAD_KEY, '1');
            this.log('webrtc.reload');
            location.reload();

            return false;
        }

        this.log('webrtc.missing', { userAgent: navigator.userAgent });
        this.showFix(
            'Falta o WebRTC nesta máquina.',
            App.isLinux() ? App.REMEDIO.webrtc : null,
        );

        return false;
    }

    /**
     * Uma peça do sistema está faltando, e aqui está o que fazer.
     *
     * Sem o comando na tela a pessoa fica com "instale as dependências", que não é
     * informação — foi assim que uma instalação que já tinha tudo passou por falta de
     * biblioteca.
     */
    showFix(problem, command) {
        el('update-screen').hidden = false;
        el('update-status').textContent = command
            ? `${problem} Rode isto no terminal:`
            : `${problem} Reinstale o Unkvoid por cima para repor o que falta.`;

        el('fix-block').hidden = ! command;

        if (! command) {
            return;
        }

        el('fix-command').textContent = command;
        el('fix-copy').onclick = async () => {
            try {
                await navigator.clipboard.writeText(command);
                el('fix-copy').textContent = 'Copiado!';
            } catch {
                // Sem área de transferência o texto continua na tela para copiar à mão.
                el('fix-copy').textContent = 'Copie à mão';
            }
        };
    }

    /**
     * Quanto já baixou, na tela.
     *
     * Sem isto ela fica parada em "Procurando atualizações…" durante todo o download, e
     * a leitura correta de uma tela parada é "travou".
     */
    showDownloadProgress() {
        void listen('update:progress', ({ payload: [downloaded, total] }) => {
            el('update-bar').hidden = false;

            if (! total) {
                // Servidor que não manda `content-length`: sem tamanho não há fração, e
                // uma barra chutando porcentagem mentiria. Fica o quanto já veio.
                el('update-status').textContent =
                    `Baixando a atualização… ${(downloaded / 1024 / 1024).toFixed(1)} MB`;

                return;
            }

            const percent = Math.min(100, Math.round((downloaded / total) * 100));

            el('update-status').textContent = `Baixando a atualização… ${percent}%`;
            el('update-fill').style.width = `${percent}%`;
        });
    }

    /**
     * A janela nasce pequena, do tamanho de um diálogo de carregamento, e só cresce
     * quando o app está pronto para ser usado.
     *
     * Falhar aqui não pode prender ninguém: a janela é redimensionável desde o começo,
     * então o pior caso é abrir pequena e a pessoa arrastar a borda.
     */
    async expandWindow() {
        try {
            await invoke('expand_window');
        } catch (failure) {
            this.log('window.expand.error', { message: failure.message ?? String(failure) });
        }
    }

    /**
     * Procura, baixa e instala a versão nova — sem perguntar nada.
     *
     * Roda na abertura e de tempos em tempos: o app fica dias aberto na bandeja, então
     * só olhar na abertura significaria esperar o próximo reinício da máquina para ver
     * uma versão publicada hoje. (Iniciar junto com o sistema não existe: o plugin de
     * autostart foi removido, e este comentário dizia o contrário.)
     */
    async update() {
        // No Linux quem atualiza é o APT. Deixar os dois caminhos ligados faria o app
        // pedir senha de root com `pkexec` no meio da abertura para fazer o que o
        // `apt upgrade` já faz junto com o resto do sistema.
        if (App.isLinux()) {
            return;
        }

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
            const label = el('room-code').value.trim().toLowerCase();
            void this.enterRoom(label || newRoomCode());
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
            this.sfu.addEventListener('reconnecting', event => this.log('sfu.reconnecting', event.detail));
            this.sfu.addEventListener('reconnected', event => void this.afterReconnect(event.detail));
            // Sem isto, desistir de reconectar era uma tela parada e nenhuma palavra.
            this.sfu.addEventListener('closed', () => this.fail('a conexão caiu e não voltou. Saia e entre na sala de novo.'));
            this.sfu.addEventListener('newProducer', event => this.consume(event.detail));
            this.sfu.addEventListener('peersChanged', () => this.refreshPeople());
            this.sfu.addEventListener('peerLeft', event => this.showScreen(event.detail.peerId, null));
            this.sfu.addEventListener('producerDead', event => void this.broadcastDied(event.detail));
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

            // O app transmite H.264. Se o WebKit desta máquina não anuncia o codec, o
            // servidor recusa cada `consume` e a pessoa fica olhando para uma sala vazia
            // sem um único erro na tela. É o modo de falha mais caro do Linux.
            if (! this.sfu.videoCodecs.some(codec => /h264/i.test(codec))) {
                this.log('device.h264.missing', { codecs: this.sfu.videoCodecs });
                this.fail(App.isLinux()
                    ? `sem H.264 nesta máquina — rode: ${App.REMEDIO.h264}`
                    : 'esta máquina não decodifica H.264, e é assim que as telas chegam');
            }

            this.refreshPeople();
            this.peopleStatsTimer = setInterval(() => {
                void this.refreshPeopleStats();
            }, 2000);
        } catch (failure) {
            this.fail(`não deu para entrar na sala: ${failure.message}`);
            await this.leave();
        }
    }

    wireRoom() {
        el('copy-code').onclick = () => this.copyCode();
        el('room-people').onclick = () => {
            const list = el('people-list');
            list.hidden = ! list.hidden;
            if (! list.hidden) {
                this.drawPeopleList();
            }
        };
        el('layout').onclick = () => this.toggleLayout();
        el('share').onclick = () => this.openShareModal();
        el('stop').onclick = () => this.stopSharing();
        el('self-view').onclick = () => this.toggleSelfView();
        el('watch-pending').onclick = () => this.refreshWatch();
        el('people-refresh').onclick = () => this.refreshWatch();
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
        this.paintPing();

        this.paintWatchPrompt();

        if (! el('people-list').hidden) {
            this.drawPeopleList();
        }
    }

    drawPeopleList() {
        const list = el('people-list-items');
        list.innerHTML = '';

        const people = [...(this.sfu?.peers?.values() ?? [])];

        for (const person of people) {
            const item = document.createElement('div');

            item.className = 'flex items-center gap-2 rounded px-2 py-1.5 text-sm text-white';
            item.innerHTML = '<span class="size-2 shrink-0 rounded-full bg-emerald-400"></span>'
                + '<span class="min-w-0 flex-1 truncate"></span>'
                + '<span class="min-w-0 shrink truncate text-xs text-ink-soft"></span>'
                + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-xs text-white ring-1 ring-inset ring-line hover:bg-line" data-watch type="button" hidden>Assistir</button>'
                + '<button class="rounded px-1.5 py-0.5 text-xs text-danger hover:bg-line" data-remove type="button" hidden>Remover</button>';
            item.querySelectorAll('span')[1].textContent = person.name;
            const info = person.reconnecting
                ? 'parado'
                : `${this.sfu?.peerLatency?.get(person.peerId) ?? '--'} ms`;
            item.querySelectorAll('span')[2].textContent = person.sharing ? `compartilhando · ${info}` : info;
            item.querySelector('span').classList.toggle('bg-danger', Boolean(person.reconnecting));
            const remove = item.querySelector('[data-remove]');
            remove.hidden = !person.reconnecting || person.self;
            remove.onclick = () => this.removeStoppedPeer(person.peerId);

            // Transmitindo e sem quadro na tela: o `newProducer` se perdeu, ou o consumo
            // falhou. Sem este botao a unica saida era sair da sala e entrar de novo.
            const watch = item.querySelector('[data-watch]');
            watch.hidden = ! this.missingScreen(person);
            watch.onclick = () => void this.watchPeer(person.peerId);

            list.appendChild(item);
        }

        if (! people.length) {
            list.innerHTML = '<p class="text-sm text-ink-soft">Nenhuma pessoa conectada.</p>';
        }

    }

    async refreshPeopleStats() {
        await this.sfu?.updatePeerLatency?.();
        this.refreshPeople();
    }

    /**
     * O ping na barra.
     *
     * Ele vivia só dentro do relógio das estatísticas da transmissão, então quem entrava
     * numa sala e não compartilhava nada lia `ping --` para sempre — inclusive quem
     * acabou de criar a sala, que é justamente quem quer saber se o servidor responde.
     */
    paintPing() {
        const ping = this.sfu?.transportRttMs ?? this.sfu?.lastRttMs;

        const parts = [ping == null ? '--' : `${ping} ms`];

        if (this.broadcastLine) {
            parts.push(this.broadcastLine);
        }

        document.querySelector('[data-broadcast-stats]').textContent = parts.join(' · ');
    }

    /**
     * Voltou a falar com o servidor.
     *
     * Retomada mantém transportes, producers e consumers vivos: não há nada a fazer.
     * Sem retomada a sessão é outra — peerId novo, nenhum consumer — e os quadros na
     * tela viraram retrato de uma conexão que não existe mais. Antes ninguém escutava
     * este evento, então a tela ficava congelada e o botão de assistir continuava
     * escondido, porque o elemento antigo ainda estava lá.
     */
    async afterReconnect({ resumed, peers }) {
        this.log('sfu.reconnected', { resumed, peers: peers?.length ?? 0 });

        if (resumed) {
            return;
        }

        for (const tile of [...el('stage').children]) {
            this.showScreen(tile.dataset.screen, null);
        }

        this.selfStream = null;

        for (const peer of peers ?? []) {
            for (const producer of peer.producers ?? []) {
                await this.consume({ producerId: producer.producerId, peerId: peer.peerId });
            }
        }

        this.refreshPeople();
    }

    async removeStoppedPeer(peerId) {
        try {
            el('people-list').hidden = true;
            await this.sfu.request('removePeer', { peerId });
            this.log('peer.removed', { peerId });
        } catch (error) {
            this.fail(`não foi possível remover: ${error.message ?? error}`);
        }
    }

    /** Esta compartilhando e mesmo assim nao ha nada desenhado por ela. */
    missingScreen(peer) {
        return Boolean(peer?.sharing) && ! document.querySelector(`[data-screen="${peer.peerId}"]`);
    }

    /** Acende o botao de fora quando alguem transmite e a tela nao abriu sozinha. */
    paintWatchPrompt() {
        const pending = [...(this.sfu?.peers?.values() ?? [])].some(peer => this.missingScreen(peer));

        el('watch-pending').hidden = ! pending;
        el('stage-empty').querySelector('p').textContent = pending
            ? 'Alguém está compartilhando, mas a tela não abriu sozinha.'
            : 'Ninguém está compartilhando ainda.';
    }

    /** Pega tudo o que uma pessoa publica e ainda nao esta na tela. */
    async watchPeer(peerId) {
        const peer = this.sfu?.peers?.get(peerId);

        for (const producer of peer?.producers ?? []) {
            await this.consume({ producerId: producer.producerId, peerId });
        }

        this.paintWatchPrompt();
    }

    /** O mesmo, para a sala inteira: o "atualizar" de quem abriu e nao viu nada. */
    async refreshWatch() {
        for (const peerId of [...(this.sfu?.peers?.keys() ?? [])]) {
            await this.watchPeer(peerId);
        }

        this.refreshPeople();
    }

    /**
     * Ver a propria transmissao, sem som.
     *
     * Vem do servidor como a de qualquer um: e a unica prova de que a sala esta mesmo
     * recebendo alguma coisa. O audio fica de fora de proposito — devolver o som do jogo
     * pela mesma maquina que o capturou e microfonia garantida.
     *
     * Esconder pausa no servidor em vez de descartar o consumer: o SFU nao tem acao de
     * fechar consumer, e pausado ele ja para de gastar banda.
     * ponytail: se um dia houver `closeConsumer`, esconder deveria fechar de vez.
     */
    async toggleSelfView() {
        const peerId = this.sfu?.peerId;
        const producerId = this.broadcast?.videoProducerId;

        if (! peerId || ! producerId) {
            return;
        }

        try {
            if (document.querySelector(`[data-screen="${peerId}"]`)) {
                await this.sfu.setPeerPaused(peerId, true);
                this.showScreen(peerId, null);
                el('self-view').textContent = 'Ver o que a sala vê';

                return;
            }

            if (this.selfStream) {
                await this.sfu.setPeerPaused(peerId, false);
                this.showScreen(peerId, this.selfStream);
            } else {
                await this.consume({ producerId, peerId });
                this.selfStream = document.querySelector(`[data-screen="${peerId}"] video`)?.srcObject ?? null;
            }

            el('self-view').textContent = 'Ocultar minha tela';
        } catch (failure) {
            this.fail(`não deu para ver a própria transmissão: ${failure.message ?? failure}`);
        }
    }

    /**
     * Para de receber sem sair da sala.
     *
     * Pausar so o `<video>` continuaria baixando e decodificando: o custo esta no
     * decoder, e ele so descansa quando o pacote deixa de chegar.
     */
    async togglePause(peerId) {
        const paused = ! this.pausedPeers.has(peerId);
        const tile = document.querySelector(`[data-screen="${peerId}"]`);
        const video = tile?.querySelector('video');
        const audio = this.remoteAudios.get(peerId);

        try {
            await this.sfu.setPeerPaused(peerId, paused);
        } catch (failure) {
            this.fail(`não deu para ${paused ? 'pausar' : 'retomar'}: ${failure.message ?? failure}`);

            return;
        }

        if (paused) {
            this.pausedPeers.add(peerId);
            video?.pause();
            audio?.pause();
        } else {
            this.pausedPeers.delete(peerId);
            void video?.play().catch(() => 0);
            void audio?.play().catch(() => 0);
        }

        const toggle = tile?.querySelector('[data-pause]');

        if (toggle) {
            toggle.textContent = paused ? 'Retomar' : 'Pausar';
        }

        this.log('media.paused', { peerId, paused });
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
                const tile = document.querySelector(`[data-screen="${peerId}"]`);
                if (tile) {
                    this.attachAudioControl(peerId, tile);
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
        const existing = document.querySelector(`[data-screen="${from}"]`);

        if (! stream) {
            // Tirar o elemento não para o decoder: a faixa segue viva no transporte, e
            // o app fica dias aberto. Cada transmissão encerrada deixava mais uma.
            for (const media of [existing?.querySelector('video'), this.remoteAudios.get(from)]) {
                media?.srcObject?.getTracks?.().forEach(track => track.stop());
            }

            existing?.remove();
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

        const tile = existing ?? document.createElement('figure');

        tile.dataset.screen = from;
        tile.innerHTML = '<video class="min-h-0 w-full flex-1 bg-black object-contain" autoplay playsinline></video>'
            + '<figcaption class="flex items-center gap-2 bg-panel px-3 py-1.5 text-xs text-ink">'
            + '<span class="truncate"></span>'
            + '<span class="text-ink-dim" data-media-stats>buffer -- · fps --</span>'
            + '<span class="flex-1"></span>'
            + '<span class="flex items-center gap-1.5 text-ink-soft" data-audio-control hidden>'
            + '<button class="cursor-pointer rounded px-1 hover:text-white" data-audio-mute type="button" title="Mutar">🔊</button>'
            + '<input class="w-20 accent-brand" data-audio-volume type="range" min="0" max="100" value="100" aria-label="Volume desta transmissão">'
            + '<span data-audio-volume-value>100%</span>'
            + '</span>'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-pause type="button">Pausar</button>'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-focus type="button">Focar</button>'
            + '<span class="flex items-center gap-1.5">'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-fullscreen type="button">Tela cheia</button>'
            + '</span>'
            + '</figcaption>';

        const video = tile.querySelector('video');
        video.srcObject = stream;
        this.attachAudioControl(from, tile);
        video.onerror = () => this.log('media.video.error', {
            peerId: from,
            message: video.error?.message ?? `media error ${video.error?.code ?? 'unknown'}`,
        });
        video.onstalled = () => this.log('media.video.stalled', { peerId: from });
        video.onwaiting = () => this.log('media.video.waiting', { peerId: from });
        video.onended = () => this.log('media.video.ended', { peerId: from });
        const owner = this.sfu?.peers?.get(from);

        tile.querySelector('span').textContent = owner?.self ? `${owner.name} (você, sem som)` : owner?.name ?? 'transmitindo';

        const toggle = tile.querySelector('[data-pause]');

        toggle.textContent = this.pausedPeers.has(from) ? 'Retomar' : 'Pausar';
        toggle.onclick = () => void this.togglePause(from);
        tile.querySelector('[data-focus]').onclick = () => this.focus(from);
        tile.querySelector('[data-fullscreen]').onclick = () => this.toggleFullscreen(tile, video, from);

        if (! existing) {
            el('stage').appendChild(tile);
        }

        this.startMediaStats(from, video);
        this.paintLayout();
    }

    /**
     * Tela cheia, tentando de verdade em vez de perguntar se o método existe.
     *
     * A versão anterior só caía para o próximo candidato quando a função **não existia**.
     * No WebKitGTK ela existe e a promessa é rejeitada — "The object is in an invalid
     * state" ao pedir num `<figure>` — então o primeiro erro ia direto para a mensagem
     * de falha e os outros caminhos nunca eram tentados.
     */
    async toggleFullscreen(tile, video, peerId) {
        if (document.fullscreenElement ?? document.webkitFullscreenElement) {
            await (document.exitFullscreen?.() ?? document.webkitExitFullscreen?.());

            return;
        }

        // O elemento de vídeo antes do cartão: é o que todo motor aceita. O último é o
        // do iOS, que não devolve promessa nenhuma.
        const candidates = [
            [video, 'requestFullscreen'],
            [video, 'webkitRequestFullscreen'],
            [tile, 'requestFullscreen'],
            [tile, 'webkitRequestFullscreen'],
            [video, 'webkitEnterFullscreen'],
        ];

        const failures = [];

        for (const [target, method] of candidates) {
            if (typeof target[method] !== 'function') {
                continue;
            }

            try {
                await target[method]();
                this.log('media.fullscreen.ok', { peerId, method });

                return;
            } catch (failure) {
                failures.push(`${method}: ${failure.message ?? failure}`);
            }
        }

        this.log('media.fullscreen.error', { peerId, failures });
        this.fail(`não foi possível abrir tela cheia: ${failures[0] ?? 'nenhum modo suportado'}`);
    }

    attachAudioControl(peerId, tile) {
        const audio = this.remoteAudios.get(peerId);
        const control = tile.querySelector('[data-audio-control]');

        if (! audio || ! control || control.dataset.ready === 'true') {
            if (audio && control) {
                control.hidden = false;
            }

            return;
        }

        const input = control.querySelector('[data-audio-volume]');
        const value = control.querySelector('[data-audio-volume-value]');
        const mute = control.querySelector('[data-audio-mute]');

        // `muted` do próprio elemento guarda o volume: desmutar devolve a mesma % sem
        // ninguém aqui precisar lembrar dela.
        const paint = () => {
            const volume = Math.round(audio.volume * 100);

            input.value = String(volume);
            mute.textContent = audio.muted || volume === 0 ? '🔇' : '🔊';
            mute.title = audio.muted ? 'Ativar o som' : 'Mutar';
            value.textContent = audio.muted ? 'mudo' : `${volume}%`;
        };

        input.oninput = event => {
            const next = Number(event.target.value);

            audio.volume = next / 100;
            audio.muted = next === 0;
            paint();
            this.log('media.audio.volume', { peerId, volume: next / 100, muted: audio.muted });
        };
        mute.onclick = () => {
            audio.muted = ! audio.muted;
            paint();
            this.log('media.audio.mute', { peerId, muted: audio.muted });
        };
        paint();
        control.dataset.ready = 'true';
        control.hidden = false;
    }

    startMediaStats(peerId, video) {
        clearInterval(this.mediaStatsTimers.get(peerId));

        let frames = 0;
        let lastFrames = 0;
        let lastSample = performance.now();
        const refreshPreview = () => {
            const tile = document.querySelector(`[data-screen="${peerId}"]`);
            const stats = tile?.querySelector('[data-media-stats]');

            if (! tile || ! stats) {
                clearInterval(this.mediaStatsTimers.get(peerId));
                this.mediaStatsTimers.delete(peerId);

                return;
            }

            if (this.pausedPeers.has(peerId)) {
                stats.textContent = 'pausado';

                return;
            }

            const now = performance.now();
            const elapsedMs = Math.max(now - lastSample, 1);
            const buffer = video.buffered.length
                ? Math.max(0, video.buffered.end(video.buffered.length - 1) - video.currentTime)
                : 0;
            const fps = Math.round((frames - lastFrames) * 1000 / elapsedMs);
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
            lastSample = now;
        };

        const countFrame = () => {
            frames += 1;
            if ('requestVideoFrameCallback' in video) {
                video.requestVideoFrameCallback(countFrame);
            }
        };

        if ('requestVideoFrameCallback' in video) {
            video.requestVideoFrameCallback(countFrame);
        } else {
            const count = () => { frames += 1; };
            video.addEventListener('timeupdate', count);
        }

        const timer = setInterval(refreshPreview, 1000);
        this.mediaStatsTimers.set(peerId, timer);
        refreshPreview();
    }

    /** Uma tela ocupando tudo, ou de volta para a grade. */
    focus(from) {
        this.focused = this.focused === from ? null : from;
        this.paintLayout();
    }

    toggleLayout() {
        const first = el('stage').firstElementChild?.dataset.screen ?? null;

        this.focused = this.focused ? null : first;
        this.paintLayout();
    }

    /**
     * Grade ou foco, e o palco só aparece quando há o que mostrar.
     *
     * As colunas saem da raiz do total: 1 tela ocupa tudo, 2 a 4 ficam em 2 colunas, 5 a
     * 9 em 3. Fixar em 2 colunas deixava cinco telas em fileiras finas e ilegíveis.
     */
    paintLayout() {
        const tiles = [...el('stage').children];

        el('stage').hidden = ! tiles.length;
        el('stage-empty').hidden = Boolean(tiles.length);
        el('layout-icon').innerHTML = this.focused ? FOCUS_ICON : GRID_ICON;

        const columns = Math.ceil(Math.sqrt(tiles.length || 1));

        el('stage').style.gridTemplateColumns = `repeat(${this.focused ? 1 : columns}, minmax(0, 1fr))`;
        this.paintWatchPrompt();

        for (const tile of tiles) {
            const hidden = Boolean(this.focused) && tile.dataset.screen !== this.focused;

            tile.className = LOOK.tile;
            tile.hidden = hidden;
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

        for (const tab of document.querySelectorAll('[data-tab]')) {
            tab.onclick = () => this.drawShareTab(tab.dataset.tab);
        }

        el('share-audio').onchange = () => this.paintAudioOptions();
        el('mute-calls').onchange = () => this.paintAudioOptions();
        this.paintAudioOptions();

        const [displays, appWindows] = await Promise.all([
            invoke('list_displays').catch(() => []),
            invoke('list_windows').catch(() => []),
        ]);

        this.shareSources = {
            display: displays.map(display => ({
                value: `display:${display.id}`,
                label: `Tela ${display.id}`,
                detail: `${display.width}×${display.height}`,
            })),
            // Janela sem título é painel de sistema: mostrar só polui a escolha.
            window: appWindows
                .filter(appWindow => appWindow.title.trim())
                .slice(0, App.MAX_WINDOW_SOURCES)
                .map(appWindow => ({
                    value: `window:${appWindow.id}`,
                    label: appWindow.title,
                    detail: appWindow.application,
                })),
        };

        this.drawShareTab('display');
    }

    /**
     * O que dá e o que não dá em áudio, dito antes de transmitir.
     *
     * Sem isto a pessoa marca "sem o áudio do Discord", compartilha a tela inteira no
     * Windows, e a conversa vai junto mesmo assim — sem nada na tela explicando por quê.
     * O sistema só deixa excluir uma árvore de processos por captura, e ela já é a nossa.
     */
    paintAudioOptions() {
        const audio = el('share-audio').checked;
        const isWindows = /Win/i.test(navigator.platform);
        const isDisplay = ! this.shareSource || this.shareSource.startsWith('display:');

        el('mute-calls').disabled = ! audio;

        const note = App.isLinux()
            ? 'O Linux ainda não captura áudio do sistema: a transmissão vai sem som.'
            : audio && el('mute-calls').checked && isWindows && isDisplay
                ? 'Na tela inteira o Windows não separa o áudio por aplicativo. Escolha a janela do jogo em Aplicativos para deixar o Discord de fora.'
                : '';

        el('audio-note').textContent = note;
        el('audio-note').hidden = ! note;
    }

    /**
     * Uma aba por vez, com miniatura de cada item.
     *
     * O nome sozinho não basta: duas janelas chamadas "Terminal" são indistinguíveis, e
     * escolher errado manda para a sala o que a pessoa não queria mostrar.
     */
    drawShareTab(tab) {
        const list = el('share-sources');
        const items = this.shareSources?.[tab] ?? [];

        for (const tab of document.querySelectorAll('[data-tab]')) {
            const active = tab.dataset.tab === tab;

            tab.classList.toggle('border-brand', active);
            tab.classList.toggle('text-white', active);
            tab.classList.toggle('border-transparent', ! active);
            tab.classList.toggle('text-ink-soft', ! active);
        }

        this.shareSource = null;
        el('share-confirm').disabled = true;
        list.innerHTML = '';

        if (! items.length) {
            const reason = navigator.platform.startsWith('Linux')
                ? 'Compartilhar a tela ainda não funciona no Linux. Dá para assistir quem transmite.'
                : 'Nenhuma tela encontrada. No macOS, autorize a gravação de tela nas Configurações do Sistema.';

            list.innerHTML = tab === 'display'
                ? `<p class="text-sm text-ink-soft">${reason}</p>`
                : '<p class="text-sm text-ink-soft">Nenhuma janela aberta para compartilhar.</p>';

            return;
        }

        list.className = 'mt-4 grid min-h-0 flex-1 auto-rows-min grid-cols-2 content-start gap-3 overflow-y-auto';

        for (const item of items) {
            const button = document.createElement('button');

            button.type = 'button';
            button.dataset.source = item.value;
            button.className = 'cursor-pointer overflow-hidden rounded-lg border-2 border-transparent bg-rail text-left transition-colors hover:border-brand';
            button.innerHTML = '<div class="flex aspect-video items-center justify-center bg-black">'
                + '<img class="size-full object-contain" alt="" hidden>'
                + '<span class="text-xs text-ink-dim">sem prévia</span>'
                + '</div>'
                + '<div class="px-2.5 py-2">'
                + '<p class="truncate text-sm text-white"></p>'
                + '<p class="truncate text-xs text-ink-soft"></p>'
                + '</div>';

            const [label, detail] = button.querySelectorAll('p');

            label.textContent = item.label;
            detail.textContent = item.detail ?? '';
            button.onclick = () => this.pickShareSource(button);
            list.appendChild(button);

            // Limita previews de aplicativos: cada uma inicia uma captura nativa e
            // muitas janelas ao mesmo tempo congelam o seletor no Windows.
            if (tab === 'window' && items.indexOf(item) >= App.MAX_WINDOW_PREVIEWS) {
                continue;
            }

            const refreshPreview = async () => {
                if (this.previewInFlight.has(item.value)) {
                    return;
                }

                this.previewInFlight.add(item.value);

                try {
                    const data = await invoke('source_preview', { source: item.value });

                    if (! data || ! button.isConnected) {
                        return;
                    }

                    const image = button.querySelector('img');

                    image.src = data;
                    image.hidden = false;
                    button.querySelector('span').hidden = true;
                } catch {
                    // A janela pode desaparecer enquanto o seletor está aberto.
                } finally {
                    this.previewInFlight.delete(item.value);
                }
            };

            void refreshPreview();
        }
    }

    /**
     * Marca o escolhido pela borda, não pelo fundo: o card é quase todo miniatura, e
     * pintar o fundo não aparece atrás da imagem.
     */
    pickShareSource(button) {
        for (const other of el('share-sources').querySelectorAll('button')) {
            const chosen = other === button;

            other.classList.toggle('border-brand', chosen);
            other.classList.toggle('border-transparent', ! chosen);
        }

        this.shareSource = button.dataset.source;
        el('share-confirm').disabled = false;
        this.paintAudioOptions();
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
        const audio = el('share-audio').checked;
        const muteCalls = audio && el('mute-calls').checked;

        this.log('broadcast.start', {
            quality: el('quality').value,
            fps: el('fps').value,
            source: this.shareSource,
            audio,
            muteCalls,
        });
        try {
            await this.broadcast.start(
                el('quality').value,
                Number(el('fps').value),
                this.shareSource,
                audio,
                muteCalls,
            );
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
        el('self-view').hidden = ! on;
        if (! on) {
            // A propria tela some junto: o servidor nao manda `producerClosed` para quem
            // fechou o producer, entao o quadro ficaria congelado para sempre.
            if (this.sfu?.peerId) {
                this.showScreen(this.sfu.peerId, null);
            }

            this.selfStream = null;
            el('self-view').textContent = 'Ver o que a sala vê';
            clearInterval(this.broadcastStatsTimer);
            this.broadcastStatsTimer = null;
            this.lastBroadcastStats = null;
            this.broadcastStatsAt = 0;
            this.broadcastLine = null;
            this.paintPing();
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
        const ping = this.sfu?.transportRttMs ?? this.sfu?.lastRttMs ?? '--';
        const seconds = elapsed / 1000;

        // O que sai da máquina, não o que a captura produz: tela parada entrega quadro
        // "sem mudança" sem buffer nenhum, e contar capturas mostrava 45 fps enquanto a
        // sala recebia 2.
        this.broadcastLine = previous
            ? [
                `${Math.round((stats.sent - previous.sent) / seconds)} fps`,
                `${((stats.sentBytes - previous.sentBytes) * 8 / seconds / 1e6).toFixed(1)} Mb/s`,
                ...(stats.sendDropped ? [`${stats.sendDropped} perdidos`] : []),
            ].join(' · ')
            : 'transmitindo…';

        this.paintPing();
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

    /**
     * Trinta segundos sem um pacote chegar ao servidor, e ele fechou a transmissão.
     *
     * Sem isto o app fica com o botão "Parar" na tela e a sala inteira vendo preto, que é
     * exatamente o modo de falha que ninguém consegue diagnosticar: aqui todo contador
     * marca saúde porque o socket aceitou os bytes — eles é que não chegam do outro lado.
     */
    async broadcastDied(detail) {
        this.log('broadcast.dead', detail);

        if (! this.sharing) {
            return;
        }

        await this.stopSharing();
        this.fail('a transmissão não chegou ao servidor: nenhum pacote entrou em 30 s. A porta de RTP está bloqueada no caminho.');
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
        clearInterval(this.peopleStatsTimer);
        this.peopleStatsTimer = null;

        this.sfu = null;
        this.broadcast = null;
        this.room = null;
        this.remoteAudios.clear();
        this.pausedPeers.clear();
        this.selfStream = null;
        this.focused = null;
        el('people-list').hidden = true;

        el('stage').innerHTML = '';
        el('room-error').hidden = true;
        document.querySelectorAll('audio[data-remote]').forEach(audio => audio.remove());

        this.showEntry();
    }

    log(event, data = {}) {
        const line = `${new Date().toISOString()} ${event} ${JSON.stringify(data)}`;

        // Também em disco, pelo Rust. Aqui dentro o diagnóstico vive na memória da
        // webview e com teto de linhas: se o app cai, ele cai junto — que é justamente
        // quando alguém precisa dele. O erro é engolido de propósito, porque falhar ao
        // registrar não pode derrubar o que estava sendo registrado.
        invoke('log_line', { line }).catch(() => null);

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

    async openLogs() {
        el('logs-modal').hidden = false;
        this.renderLogs();

        // O arquivo tem o que a janela não tem: o que o Rust registrou e o que sobrou de
        // uma execução que terminou em crash.
        const path = await invoke('log_path').catch(() => '');

        el('logs-path').textContent = path
            ? `Arquivo completo, inclusive de execuções que travaram: ${path}`
            : 'Copie estes eventos após reproduzir o problema.';
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
