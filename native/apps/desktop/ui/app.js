import { SfuClient } from './SfuClient.js';

import { Broadcast } from './broadcast.js';
import { isRoomCode, newRoomCode } from './room-code.js';

const el = id => document.getElementById(id);

const GRID_ICON = '<path stroke-linecap="round" stroke-linejoin="round" d="M4 5h6v6H4zM14 5h6v6h-6zM4 13h6v6H4zM14 13h6v6h-6z"/>';
const FOCUS_ICON = '<path stroke-linecap="round" stroke-linejoin="round" d="M4 5h16v10H4zM4 17h4v2H4zM10 17h4v2h-4zM16 17h4v2h-4z"/>';

/** Aparência dos elementos que o JavaScript cria. */
/** Saturação inicial, em porcentagem. Ver o comentário de `attachVideoConfig`. */
const SATURATION_DEFAULT = 115;

const LOOK = {
    tile: 'group relative m-0 flex min-h-0 flex-col overflow-hidden rounded-lg bg-black',
    tileFocused: 'row-span-full col-span-full',

    /* Fora do fluxo e acima de tudo: é assim que o vídeo cobre a janela inteira sem a
       barra da sala nem o respiro do `body` sobrando na borda. O arredondamento do
       cartão fica: com a legenda em `absolute`, o vídeo é o único filho no fluxo e
       ocupa a altura toda, então a única borda que sobra é a das proporções. */
    tileFullscreen: 'fixed inset-0 z-40',

    caption: 'flex items-center gap-2 bg-panel px-3 py-1.5 text-xs text-ink',

    /* Em tela cheia a legenda flutua por cima do vídeo. Quem a mostra e a esconde é o
       relógio de ociosidade, não o `group-hover`: com o cartão ocupando a janela inteira
       o ponteiro está sempre em cima dele, então o hover ficaria ligado para sempre. */
    captionFullscreen: 'absolute inset-x-0 bottom-0 z-50 bg-panel/85 transition-opacity duration-200',

    /* A barra da sala precisa de `fixed` e de z acima do cartão para continuar
       alcançável: o vídeo é `fixed inset-0 z-40` e cobriria qualquer coisa no fluxo. */
    headerFullscreen: 'fixed inset-x-0 top-0 z-50 flex h-12 items-center gap-3 bg-panel/85 px-3 transition-opacity duration-200',
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

    /**
     * Ponteiro parado por este tanto em tela cheia e some tudo: barra, legenda e o
     * próprio cursor. Três segundos é o que separa "parou de mexer" de "está a caminho
     * do botão".
     */
    static IDLE_MS = 3000;

    static INSTALL_KEY = 'unkvoid.instalacao';
    static TOAST_MS = 6000;

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
    static isLinux() {
        return /Linux/i.test(navigator.platform) || /Linux/i.test(navigator.userAgent);
    }

    /**
     * O único servidor. VITE_SERVER aponta um build local para uma pilha local, e o
     * override no localStorage serve para cutucar um build já pronto sem recompilar.
     */
    static SERVER = import.meta.env.VITE_SERVER ?? localStorage.getItem('server') ?? 'https://unkvoid.com';

    /** A sinalização mora no mesmo host, atrás do mesmo TLS. */
    /**
     * O identificador desta instalação do app.
     *
     * Sobrevive a reconectar e a fechar o app, que é o que o `peerId` não faz — ele é
     * sorteado a cada conexão. É nele que a posse da sala se apoia: dono que perde a sala
     * quando a internet oscila não é dono de nada.
     *
     * Não é prova de identidade. Quem editar o próprio app manda o que quiser, e vale
     * exatamente o que o código da sala vale: quem tem a string, entra. É o modelo do
     * produto, e está registrado aqui para ser escolha e não acidente.
     */
    static installId() {
        let id = localStorage.getItem(App.INSTALL_KEY);

        if (! id) {
            id = crypto.randomUUID();
            localStorage.setItem(App.INSTALL_KEY, id);
        }

        return id;
    }

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
        this.mediaStatsRuns = new Map();
        this.remoteAudios = new Map();
        // Quem está sendo assistido pelo caminho nativo (Linux): uma janela por pessoa.
        this.nativeWatching = new Set();
        this.consumingProducers = new Set();
        this.peopleStatsTimer = null;

        /** Quem esta pausado nao gasta banda nem decoder: o servidor para de mandar. */
        this.pausedPeers = new Set();

        /** A propria tela, guardada para o botao poder mostrar e esconder sem reconsumir. */
        this.selfStream = null;

        /** Grade mostra todos do mesmo tamanho; foco dá a tela toda a um só. */
        this.focused = null;

        /** Quem está ocupando a tela inteira, ou nada. Mora aqui porque o `paintLayout`
         *  reescreve a classe de todo cartão e apagaria um estado guardado no DOM. */
        this.fullscreen = null;
        this.idleTimer = null;
        this.idle = false;
        this.headerLook = null;

        this.attempt = 0;
        this.reconnect = null;
        this.warnedNoWebRTC = false;
    }

    /**
     * Ordem de abertura: atualizar, exigir o servidor, e só então deixar entrar. Entrar
     * num app desatualizado ou sem servidor só produziria erro mais adiante.
     */
    async start() {
        this.checkWebRTC();

        // Fora do `wireRoom`: aquele roda a cada entrada em sala, e `addEventListener`
        // soma em vez de substituir, ao contrário dos `onclick` do resto do arquivo.
        document.addEventListener('click', event => {
            const list = el('people-list');
            const target = event.target;

            if (! list.hidden && target instanceof Node
                && ! list.contains(target) && target !== el('room-people')) {
                list.hidden = true;
            }

            // O fundo escuro É o `<section>` do modal: clicar nele e não num filho quer
            // dizer que o clique caiu fora da caixa. Sem isto a única saída era o botão.
            for (const id of ['share-modal', 'logs-modal']) {
                if (target === el(id)) {
                    el(id).hidden = true;
                }
            }
        });

        // A rede caiu ou voltou enquanto a tela de reconexão estava aberta. Sem escutar,
        // o texto continuaria culpando o servidor depois de a pessoa arrancar o cabo, e
        // quem reconectou o Wi-Fi esperaria até dez segundos de espera que já não vale.
        window.addEventListener('offline', () => this.paintOffline());
        window.addEventListener('online', () => {
            if (! el('offline-screen').hidden) {
                this.attempt = 0;
                this.showOffline();
            }
        });

        // A tela cheia é nossa, não do motor: ninguém devolve o Esc de graça, e sem isto
        // a única saída seria caçar a legenda escondida no rodapé.
        document.addEventListener('keydown', event => {
            if (event.key === 'Escape' && this.fullscreen) {
                void this.toggleFullscreen(this.fullscreen);
            }
        });

        // Mexeu, tudo volta. `mousemove` chega dezenas de vezes por segundo, então o
        // trabalho por evento é reiniciar um temporizador — pintar a cada um deles
        // custaria um recálculo de estilo a cada pixel de movimento.
        document.addEventListener('mousemove', () => this.wakeUp());

        this.showDownloadProgress();

        // Procurar versão nova é a primeira coisa que o app faz ao abrir, sem depender
        // de nada além do manifesto. Antes disso, quem decidia era o `appVersion` do
        // `/health`: uma variável de ambiente na VPS que alguém tinha que lembrar de
        // subir junto com a versão. Esquecer dela deixava o app publicado hoje esperando
        // até seis horas — o tempo do temporizador — para se atualizar na abertura.
        if (await this.serverAnswered()) {
            await this.update();
        }

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
    /**
     * WebRTC só é preciso para ASSISTIR: criar sala, entrar e transmitir seguem sem ele.
     * Por isso a falta não trava o app na abertura — ela vira um aviso na hora em que
     * alguém compartilha. Antes, a tela de abertura mandava instalar pacotes do
     * GStreamer que não resolviam nada num WebKitGTK compilado sem WebRTC.
     *
     * O reload único existe porque a configuração que liga o WebRTC no Linux entra
     * depois de a primeira página nascer.
     */
    checkWebRTC() {
        if (typeof RTCPeerConnection !== 'undefined') {
            return;
        }

        if (! sessionStorage.getItem(App.WEBRTC_RELOAD_KEY)) {
            sessionStorage.setItem(App.WEBRTC_RELOAD_KEY, '1');
            this.log('webrtc.reload');
            location.reload();

            return;
        }

        this.log('webrtc.missing', { userAgent: navigator.userAgent });
    }

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
     * Procura, baixa e instala a versão nova. A única pergunta é a do sistema: no
     * Windows o instalador precisa de administrador, e o aviso do UAC é o que a pessoa
     * confirma para a instalação seguir.
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

        // Antes do download, e não depois. No Windows o `check_update` não volta: o
        // plugin dispara o instalador e encerra o processo lá dentro, então a conferência
        // que ficava depois dele nunca rodava e a transmissão caía junto com o app. A
        // versão nova espera a próxima abertura, que é quando ninguém está assistindo.
        if (this.room) {
            return;
        }

        try {
            const version = await invoke('check_update');

            if (! version) {
                return;
            }

            el('update-status').textContent = `Instalando a versão ${version}…`;

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

            return true;
        } catch {
            this.showOffline();

            return false;
        }
    }

    /**
     * Duas falhas muito diferentes mostravam a mesma tela. "Sem conexão" com a internet
     * funcionando manda a pessoa reiniciar o roteador enquanto o servidor é que está fora,
     * e ela nunca descobre que não havia nada para consertar do lado dela.
     */
    showOffline() {
        this.attempt += 1;
        this.paintOffline();
        el('offline-screen').hidden = false;

        clearTimeout(this.reconnect);
        this.reconnect = setTimeout(async () => {
            if (await this.serverAnswered()) {
                el('offline-screen').hidden = true;
                this.attempt = 0;
                this.showEntry();
            }
        }, Math.min(2000 * this.attempt, 10000));
    }

    /**
     * `navigator.onLine` só é confiável quando diz que não: sem interface de rede não há
     * o que tentar. Dizendo que sim, ainda pode ser um roteador conectado sem internet,
     * então o texto afirma só o que dá para afirmar — daqui a rede parece viva e o
     * servidor não respondeu — em vez de apontar culpado.
     */
    paintOffline() {
        const semRede = ! navigator.onLine;

        el('offline-title').textContent = semRede ? 'Sem internet' : 'Servidor sem resposta';
        el('offline-status').textContent = semRede
            ? `Este computador está sem rede. Tentando de novo… (tentativa ${this.attempt})`
            : `A sua internet está funcionando. Tentando de novo… (tentativa ${this.attempt})`;
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
            this.sfu.addEventListener('newProducer', event => {
                if (event.detail.kind === 'video') {
                    this.toast(`${this.sfu?.peers?.get(event.detail.peerId)?.name ?? 'alguém'} começou a transmitir`);
                }

                void this.consume(event.detail);
            });
            this.sfu.addEventListener('peersChanged', () => this.refreshPeople());
            this.sfu.addEventListener('peerKicked', event => this.toast(`${event.detail.name} foi removido da sala`));
            this.sfu.addEventListener('kicked', event => this.fail(event.detail?.reason ?? 'você foi removido desta sala'));
            this.sfu.addEventListener('peerJoined', event => this.toast(`${event.detail.name} entrou na sala`));
            this.sfu.addEventListener('peerLeft', event => {
                // O nome antes de remover: depois disto o `SfuClient` já esqueceu quem era.
                this.toast(`${this.sfu?.peers?.get(event.detail.peerId)?.name ?? 'alguém'} saiu da sala`);
                this.showScreen(event.detail.peerId, null);
            });
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

            const joined = await this.sfu.connect(App.socketUrl(), {
                room: this.room,
                name: this.name,
                installId: App.installId(),
            });

            this.owner = joined?.owner === true;

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
            // Sem WebRTC nenhum o `videoCodecs` nasce vazio e cairia aqui, culpando o
            // H.264 por uma falta que é do motor inteiro. Quem não tem WebRTC já é
            // avisado na hora de assistir, em `consume`.
            if (this.sfu.canWatch() && ! this.sfu.videoCodecs.some(codec => /h264/i.test(codec))) {
                // O que o motor da janela diz que sabe receber e mandar, cru: é a única
                // pista para descobrir por que o H.264 não entrou nesta distro.
                //
                // Os globais são lidos de `globalThis`: no WebKitGTK sem WebRTC eles não
                // existem, e citá-los direto derruba o `join` inteiro com "can't find
                // variable" — engolindo o aviso que esta linha existe para dar.
                this.log('device.h264.missing', {
                    codecs: this.sfu.videoCodecs,
                    receiver: globalThis.RTCRtpReceiver?.getCapabilities?.('video')?.codecs?.map(codec => codec.mimeType) ?? null,
                    sender: globalThis.RTCRtpSender?.getCapabilities?.('video')?.codecs?.map(codec => codec.mimeType) ?? null,
                    userAgent: navigator.userAgent,
                });
                this.fail('o motor da janela desta máquina não recebe H.264 pelo WebRTC. Dá para transmitir, mas não para assistir. Abra Logs e mande a linha device.h264.missing.');
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

    /**
     * Um aviso passageiro no canto.
     *
     * Existe por segurança, não por enfeite: hoje o código da sala é a única credencial,
     * e ele é digitado à mão, então nomes fáceis são adivinháveis. Alguém entrar era
     * silencioso. Um aviso não impede a entrada, mas transforma o problema invisível em
     * visível — que é o primeiro passo para alguém reagir.
     *
     * Também vai para o log, porque quem chegou depois precisa saber quem esteve na sala.
     */
    toast(message) {
        this.log('room.toast', { message });

        const card = document.createElement('p');

        card.className = 'max-w-xs rounded-lg border border-line bg-panel px-3 py-2 text-xs text-ink shadow-lg';
        card.textContent = message;
        el('toasts').appendChild(card);

        setTimeout(() => card.remove(), App.TOAST_MS);
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

        // A sessão é outra, então os producers da transmissão morreram com a antiga. A
        // captura aqui continua rodando, e sem republicar o RTP segue subindo para uma
        // porta que não existe mais, com o "ao vivo" aceso e a sala vendo preto.
        if (this.sharing) {
            try {
                await this.broadcast.republish();
                this.log('broadcast.republished', { producerId: this.broadcast.videoProducerId });
            } catch (failure) {
                this.log('broadcast.republish.error', { message: failure.message ?? String(failure) });
                await this.stopSharing();
                this.fail(`a transmissão caiu com o servidor e não voltou: ${failure.message ?? failure}`);
            }
        }

        for (const tile of [...el('stage').children]) {
            this.showScreen(tile.dataset.screen, null);
        }

        // O tile próprio foi junto com os outros e o producer é outro, então o botão não
        // pode continuar oferecendo ocultar uma tela que não está mais desenhada.
        this.selfStream = null;
        el('self-view').textContent = 'Ver o que a sala vê';

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
            this.log('peer.removed', { peerId, owner: this.owner });
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

        this.paintPaused(peerId);
        this.log('media.paused', { peerId, paused });
    }

    /**
     * Pausado precisa PARECER pausado.
     *
     * Quadro congelado e quadro travado são a mesma imagem: sem o borrão e o cinza,
     * ninguém sabe se pausou ou se a rede caiu. O filtro é do CSS, e o `data-paused` é
     * o que o liga — desenhar por cima exigiria canvas para nada.
     *
     * Também roda depois de redesenhar o cartão: o `innerHTML` nasce sem estado nenhum,
     * e uma transmissão pausada voltava nítida com o botão dizendo "Retomar".
     */
    paintPaused(peerId) {
        const tile = document.querySelector(`[data-screen="${peerId}"]`);
        const paused = this.pausedPeers.has(peerId);

        if (! tile) {
            return;
        }

        tile.toggleAttribute('data-paused', paused);
        tile.querySelector('[data-resume]').hidden = ! paused;
        tile.querySelector('[data-pause]').textContent = paused ? 'Retomar' : 'Pausar';
    }

    async consume({ producerId, peerId: ownerPeerId }) {
        // Sem WebRTC na janela (o Linux), quem recebe e desenha é o Rust com o
        // GStreamer, numa janela ao lado. Uma chamada por pessoa, não por producer.
        if (this.sfu?.canWatch?.() === false && App.isLinux()) {
            return this.consumeNative(ownerPeerId);
        }

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

            if (this.sfu?.canWatch?.() === false && ! this.warnedNoWebRTC) {
                this.warnedNoWebRTC = true;
                this.fail('este sistema não tem WebRTC no motor da janela: dá para transmitir, mas ainda não dá para assistir.');
            }
        } finally {
            this.consumingProducers.delete(producerId);
        }
    }

    /**
     * Assistir por RTP puro: o servidor manda a mídia numa porta UDP e o GStreamer
     * abre numa janela própria. O cartão no palco só diz que está acontecendo.
     */
    async consumeNative(peerId) {
        if (this.nativeWatching.has(peerId)) {
            return;
        }

        const producers = this.sfu?.peers?.get(peerId)?.producers ?? [];

        if (! producers.some(producer => producer.kind === 'video')) {
            return;
        }

        this.nativeWatching.add(peerId);
        this.log('media.native.start', { peerId });

        try {
            const keyBase64 = await invoke('watch_key');
            const srtpParameters = { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64 };
            const consumers = [];

            for (const producer of producers) {
                consumers.push(await this.sfu.request('consumePlain', { producerId: producer.producerId, srtpParameters }));
            }

            const video = consumers.find(consumer => consumer.kind === 'video');
            const audio = consumers.find(consumer => consumer.kind === 'audio');

            await invoke('watch_native', {
                peerId,
                address: `${video.ip}:${video.port}`,
                serverKey: video.srtpParameters.keyBase64,
                videoPayloadType: video.payloadType,
                audioPayloadType: audio?.payloadType ?? null,
            });

            // Só depois de o Rust ter aberto o caminho: retomar antes mandaria o
            // keyframe para um endereço que o servidor ainda não conhece.
            for (const consumer of consumers) {
                await this.sfu.request('resumeConsumer', { consumerId: consumer.consumerId });
            }

            this.showNativeTile(peerId, video.name);
            this.log('media.native.ready', { peerId });
        } catch (failure) {
            this.nativeWatching.delete(peerId);
            this.log('media.native.error', { peerId, message: failure.message ?? String(failure) });
            this.fail(`não deu para assistir: ${failure.message ?? failure}`);
        }
    }

    async stopNative(peerId) {
        if (! this.nativeWatching.has(peerId)) {
            return;
        }

        this.nativeWatching.delete(peerId);
        await invoke('stop_watch', { peerId }).catch(() => null);
    }

    showNativeTile(peerId, name) {
        const tile = document.querySelector(`[data-screen="${peerId}"]`) ?? document.createElement('figure');

        tile.dataset.screen = peerId;
        tile.innerHTML = '<span class="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-black text-center text-sm text-ink-soft">'
            + '<span>A tela está aberta numa janela separada do GStreamer.</span>'
            + '<button class="cursor-pointer rounded-md px-4 py-2 text-sm font-medium text-white ring-1 ring-inset ring-line hover:bg-line" data-native-stop type="button">Parar de assistir</button>'
            + '</span>'
            + `<figcaption class="${LOOK.caption}"><span class="truncate"></span></figcaption>`;
        tile.querySelector('figcaption span').textContent = name ?? 'alguém';
        tile.querySelector('[data-native-stop]').onclick = () => {
            void this.stopNative(peerId);
            tile.remove();
            this.paintLayout();
            this.paintWatchPrompt();
        };

        if (! tile.isConnected) {
            el('stage').appendChild(tile);
        }

        this.paintLayout();
        this.paintWatchPrompt();
    }

    /** Desenha (ou remove) a tela de quem está transmitindo. */
    showScreen(from, stream) {
        const existing = document.querySelector(`[data-screen="${from}"]`);

        if (! stream) {
            void this.stopNative(from);
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

            // Quem estava em tela cheia parou de transmitir: sem isto a janela ficava
            // ocupando o monitor inteiro mostrando o palco vazio.
            if (this.fullscreen === from) {
                void this.toggleFullscreen(from);
            }

            clearInterval(this.mediaStatsTimers.get(from));
            this.mediaStatsTimers.delete(from);
        this.mediaStatsRuns.delete(from);
            this.paintLayout();

            return;
        }

        const tile = existing ?? document.createElement('figure');

        tile.dataset.screen = from;
        tile.innerHTML = '<span class="relative flex min-h-0 flex-1">'
            + '<video class="min-h-0 w-full flex-1 bg-black object-contain" autoplay playsinline></video>'
            // O play mora por cima do vídeo, não do cartão: sobre a legenda ele cobriria
            // o botão de retomar da própria legenda.
            + '<button class="absolute inset-0 flex cursor-pointer items-center justify-center text-white/90 transition-colors hover:text-white" data-resume type="button" aria-label="Retomar esta transmissão" hidden>'
            + '<svg class="size-20 drop-shadow-lg" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14l11-7z"/></svg>'
            + '</button>'
            + '</span>'
            + `<figcaption class="${LOOK.caption}">`
            + '<span class="truncate"></span>'
            + '<span class="min-w-0 truncate text-ink-dim" data-media-stats>-- ms · -- fps</span>'
            + '<span class="flex-1"></span>'
            + '<span class="flex items-center gap-1.5 text-ink-soft" data-audio-control hidden>'
            + '<button class="cursor-pointer rounded px-1 hover:text-white" data-audio-mute type="button" title="Mutar">🔊</button>'
            + '<input class="w-20 accent-brand" data-audio-volume type="range" min="0" max="100" value="100" aria-label="Volume desta transmissão">'
            + '<span data-audio-volume-value>100%</span>'
            + '</span>'
            + '<span class="relative flex items-center">'
            + '<button class="cursor-pointer rounded px-1.5 py-0.5 text-ink-soft hover:bg-line hover:text-white" data-video-config type="button" title="Ajustes de imagem — só do seu lado, não mudam o que os outros veem">Imagem</button>'
            // Ancorado no cartão e não no corpo da página: cada transmissão tem os seus,
            // e um painel só teria de descobrir a qual delas pertence.
            + '<span class="absolute bottom-full right-0 z-30 mb-1 hidden w-56 rounded-lg border border-line bg-panel p-3 shadow-lg" data-video-panel>'
            + '<label class="flex flex-col gap-1 text-xs text-ink-soft">Brilho'
            + '<input class="accent-brand" data-brightness type="range" min="50" max="250" value="100" aria-label="Brilho desta transmissão">'
            + '</label>'
            + '<label class="mt-3 flex flex-col gap-1 text-xs text-ink-soft">Saturação'
            + '<input class="accent-brand" data-saturation type="range" min="50" max="250" value="115" aria-label="Saturação desta transmissão">'
            + '</label>'
            + '<button class="mt-3 cursor-pointer text-xs text-ink-dim hover:text-white" data-video-reset type="button">Voltar ao padrão</button>'
            + '</span>'
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
        this.attachVideoConfig(tile, video);
        video.onerror = () => this.log('media.video.error', {
            peerId: from,
            message: video.error?.message ?? `media error ${video.error?.code ?? 'unknown'}`,
        });
        video.onstalled = () => this.log('media.video.stalled', { peerId: from });
        video.onwaiting = () => this.log('media.video.waiting', { peerId: from });
        video.onended = () => this.log('media.video.ended', { peerId: from });
        const owner = this.sfu?.peers?.get(from);

        // `figcaption span` e não o primeiro `span` do cartão: o vídeo agora vem dentro
        // de um, e o nome de quem transmite ia parar em cima da imagem.
        tile.querySelector('figcaption span').textContent = owner?.self ? `${owner.name} (você, sem som)` : owner?.name ?? 'transmitindo';

        tile.querySelector('[data-pause]').onclick = () => void this.togglePause(from);
        tile.querySelector('[data-resume]').onclick = () => void this.togglePause(from);
        tile.querySelector('[data-focus]').onclick = () => this.focus(from);
        tile.querySelector('[data-fullscreen]').onclick = () => void this.toggleFullscreen(from);
        this.paintPaused(from);

        if (! existing) {
            el('stage').appendChild(tile);
        }

        this.startMediaStats(from, video);
        this.paintLayout();
    }

    /**
     * Ajustes de imagem, só para quem assiste.
     *
     * São filtros de CSS no `<video>`: nada volta para quem transmite, nada passa pelo
     * encoder, e jogo escuro ou lavado deixa de ser problema sem custar um bit a mais.
     *
     * Cada transmissão tem o seu painel, mas o valor é guardado uma vez só: quem precisa
     * clarear uma tela precisa clarear a próxima também, e ajustar tudo de novo a cada
     * pessoa que entra na sala seria pior do que não ter ajuste.
     *
     * Saturação começa acima de 100 de propósito. O H.264 em 4:2:0 joga fora três quartos
     * da informação de cor, e a imagem chega lavada em relação ao que quem transmite vê.
     */
    attachVideoConfig(tile, video) {
        // Variáveis, não `style.filter`: o borrão de pausa mora na mesma propriedade, e
        // escrever direto ali fazia um dos dois apagar o outro.
        const controls = [
            ['--brightness', tile.querySelector('[data-brightness]'), 'unkvoid.brilho', 100],
            ['--saturation', tile.querySelector('[data-saturation]'), 'unkvoid.saturacao', SATURATION_DEFAULT],
        ];

        const apply = (property, control) => {
            video.style.setProperty(property, String(Number(control.value) / 100));
        };

        for (const [property, control, key, fallback] of controls) {
            const saved = Number(localStorage.getItem(key));

            control.value = String(saved >= 50 && saved <= 250 ? saved : fallback);
            apply(property, control);

            control.oninput = () => {
                apply(property, control);
                localStorage.setItem(key, control.value);
            };
        }

        const panel = tile.querySelector('[data-video-panel]');

        tile.querySelector('[data-video-config]').onclick = () => panel.classList.toggle('hidden');
        tile.querySelector('[data-video-reset]').onclick = () => {
            for (const [property, control, key, fallback] of controls) {
                control.value = String(fallback);
                apply(property, control);
                localStorage.removeItem(key);
            }
        };
    }

    /**
     * Tela cheia de app de vídeo, não de página de navegador.
     *
     * A tela cheia do documento não serve em nenhum dos dois sistemas que reclamaram, e
     * a versão anterior tentava só ela — daí a lista de candidatos que terminava em erro.
     *
     * No Windows o WebView2 estica o vídeo dentro da janela e para por aí: a barra de
     * título continua na tela, e é exatamente isso que faz "parecer navegador".
     *
     * No macOS a API nem existe. O `macos-private-api` do Tauri está desligado no
     * `Cargo.toml`, e é ela que liga o `wry/fullscreen` — o único lugar que escreve
     * `fullScreenEnabled` nas preferências do WKWebView. Sem isso o `requestFullscreen`
     * não é função nenhuma, sobra o `webkitEnterFullscreen`, e ele rejeita com "invalid
     * state" porque vídeo de MediaStream não tem tela cheia nativa no WebKit.
     *
     * O que funciona nos três: o cartão vira `fixed inset-0` por CSS, e a janela do
     * Tauri vira tela cheia de verdade. Nenhum motor opina.
     */
    async toggleFullscreen(peerId) {
        this.fullscreen = this.fullscreen === peerId ? null : peerId;
        this.paintLayout();
        this.log('media.fullscreen', { peerId, on: Boolean(this.fullscreen) });

        try {
            // Encadeado com `?.` de propósito: o guarda da abertura só confere
            // `__TAURI__.core`, e desmontar isto no topo do arquivo deixaria a tela preta.
            await window.__TAURI__.window?.getCurrentWindow?.()?.setFullscreen(Boolean(this.fullscreen));
        } catch (failure) {
            // A janela não virou, mas o cartão já ocupa o app inteiro. Falhar na cara de
            // quem clicou seria pior do que a tela cheia pela metade que ficou.
            this.log('media.fullscreen.error', { peerId, message: failure.message ?? String(failure) });
        }
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

    /**
     * O relatório de entrada de uma transmissão.
     *
     * Sai do mesmo `getStats` que já alimenta o ping, e casa pelo SSRC igual ao
     * `updatePeerLatency`: sem esse casamento, duas telas na grade trocariam de números
     * entre si, que é pior do que não ter número nenhum.
     */
    async inboundReport(peerId) {
        const transport = this.sfu?.recvTransport;
        const consumer = this.sfu?.consumersOf(peerId)
            .map(id => this.sfu.consumers.get(id))
            .find(candidate => candidate?.kind === 'video');

        if (! transport || ! consumer) {
            return null;
        }

        const ssrc = consumer.rtpParameters?.encodings?.[0]?.ssrc;
        const stats = await transport.getStats();

        return [...stats.values()].find(report => report.type === 'inbound-rtp' && report.ssrc === ssrc) ?? null;
    }

    /**
     * Os números de quem assiste, na legenda de cada transmissão.
     *
     * Taxa e perda vêm de contadores que só sobem, então os dois são derivados do
     * intervalo: 4 Mb/s agora diz se dá para assistir, e "1.2 GB recebidos" não diz
     * nada. A perda pelo mesmo motivo é percentual do intervalo — um total que cresceu
     * num engasgo de dez minutos atrás continuaria vermelho com a rede já boa.
     *
     * Mb/s e não MB/s para bater com a linha de quem transmite, que já fala em bits.
     */
    startMediaStats(peerId, video) {
        clearInterval(this.mediaStatsTimers.get(peerId));

        let frames = 0;
        let lastFrames = 0;
        let lastInbound = null;
        let lastSample = performance.now();
        const refreshPreview = async () => {
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

            const inbound = await this.inboundReport(peerId).catch(() => null);
            const now = performance.now();
            const elapsedMs = Math.max(now - lastSample, 1);
            const seconds = elapsedMs / 1000;
            const buffer = video.buffered.length
                ? Math.max(0, video.buffered.end(video.buffered.length - 1) - video.currentTime)
                : 0;
            const fps = Math.round((frames - lastFrames) * 1000 / elapsedMs);
            const quality = video.getVideoPlaybackQuality?.();
            const ping = this.sfu?.transportRttMs ?? this.sfu?.lastRttMs;
            const since = inbound && lastInbound ? lastInbound : null;

            // `packetsLost` sabe descer — o RFC deixa o contador ser negativo, e uma
            // amostra assim viraria "-3% perdidos" na legenda.
            const lost = since ? Math.max(0, inbound.packetsLost - since.packetsLost) : 0;
            const received = since ? Math.max(0, inbound.packetsReceived - since.packetsReceived) : 0;
            const loss = since && lost + received ? lost * 100 / (lost + received) : since ? 0 : null;
            const rate = since ? (inbound.bytesReceived - since.bytesReceived) * 8 / seconds / 1e6 : null;

            stats.textContent = [
                ping == null ? '-- ms' : `${ping} ms`,
                `${fps} fps`,
                Number.isFinite(rate) ? `${rate.toFixed(1)} Mb/s` : '-- Mb/s',
                Number.isFinite(loss) ? `${loss.toFixed(1)}% perdidos` : '--% perdidos',
            ].join(' · ');

            // A legenda já disputa espaço com nome, volume, brilho e quatro botões: em
            // janela estreita ela corta, e o resto do detalhe fica aqui.
            stats.title = `buffer ${buffer.toFixed(1)} s`
                + ` · ${inbound?.packetsLost ?? '--'} pacotes perdidos no total`
                + ` · jitter ${inbound?.jitter == null ? '--' : Math.round(inbound.jitter * 1000)} ms`;

            this.log('media.stats', {
                peerId,
                pingMs: ping ?? null,
                bufferSeconds: Number(buffer.toFixed(2)),
                fps,
                mbps: Number.isFinite(rate) ? Number(rate.toFixed(2)) : null,
                lossPercent: Number.isFinite(loss) ? Number(loss.toFixed(2)) : null,
                packetsLost: inbound?.packetsLost ?? null,
                jitterMs: inbound?.jitter == null ? null : Math.round(inbound.jitter * 1000),
                framesDropped: quality?.droppedVideoFrames ?? null,
                framesDecoded: quality?.totalVideoFrames ?? null,
            });
            lastFrames = frames;
            lastInbound = inbound;
            lastSample = now;
        };

        // Cada `showScreen` redesenha o cartão e chamava isto de novo, e a corrente
        // anterior seguia se reagendando para sempre — segurando o vídeo, a closure e um
        // callback por quadro, para nada. O número da rodada é o que mata a antiga.
        const run = (this.mediaStatsRuns.get(peerId) ?? 0) + 1;

        this.mediaStatsRuns.set(peerId, run);

        const countFrame = () => {
            if (this.mediaStatsRuns.get(peerId) !== run) {
                return;
            }

            frames += 1;
            video.requestVideoFrameCallback(countFrame);
        };

        if ('requestVideoFrameCallback' in video) {
            video.requestVideoFrameCallback(countFrame);
        } else {
            const count = () => { frames += 1; };
            video.addEventListener('timeupdate', count);
        }

        const timer = setInterval(() => void refreshPreview(), 1000);
        this.mediaStatsTimers.set(peerId, timer);
        void refreshPreview();
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

        const header = document.querySelector('#room > header');

        // A aparência original é lida uma vez e guardada: reconstruí-la à mão aqui seria
        // manter a mesma lista de classes em dois lugares, e o HTML é quem manda.
        this.headerLook ??= header.className;
        header.className = this.fullscreen ? LOOK.headerFullscreen : this.headerLook;

        for (const tile of tiles) {
            // Em tela cheia o resto some de vez: um cartão da grade aparecendo atrás do
            // vídeo pelas bordas é o que faz a tela cheia parecer uma página esticada.
            const full = tile.dataset.screen === this.fullscreen;
            const hidden = this.fullscreen
                ? ! full
                : Boolean(this.focused) && tile.dataset.screen !== this.focused;

            tile.className = full ? `${LOOK.tile} ${LOOK.tileFullscreen}` : LOOK.tile;
            tile.hidden = hidden;

            const caption = tile.querySelector('figcaption');

            caption.className = full ? `${LOOK.caption} ${LOOK.captionFullscreen}` : LOOK.caption;

            // O rótulo diz a ação, não o estado: um botão escrito "Tela cheia" enquanto
            // já se está em tela cheia é a mesma armadilha do cadeado que dizia
            // "Destrancada" e trancava ao clicar.
            // O cartão do assistir nativo (Linux) não tem esse botão: a janela é do
            // GStreamer, fora do app.
            const fullscreenButton = tile.querySelector('[data-fullscreen]');

            if (fullscreenButton) {
                fullscreenButton.textContent = full ? 'Sair da tela cheia' : 'Tela cheia';
            }
        }

        this.wakeUp();
    }

    /**
     * O ponteiro mexeu: mostra tudo e recomeça a contagem.
     *
     * Fora da tela cheia isto só limpa o relógio. Deixar a contagem correndo esconderia
     * o cursor de quem saiu da tela cheia e ficou parado lendo a lista de salas.
     */
    wakeUp() {
        clearTimeout(this.idleTimer);
        this.idleTimer = null;

        // Só pinta na virada. `mousemove` chega a cada pixel, e mexer em `classList`
        // sessenta vezes por segundo para deixar tudo como já estava é trabalho jogado
        // fora bem no momento em que a máquina está codificando vídeo.
        if (this.idle) {
            this.paintIdle(false);
        }

        if (! this.fullscreen) {
            return;
        }

        this.idleTimer = setTimeout(() => this.paintIdle(true), App.IDLE_MS);
    }

    /**
     * Some com a barra, a legenda e o cursor, ou traz os três de volta.
     *
     * Um estado só para os três porque a pergunta é uma só — a pessoa parou de mexer? —
     * e três relógios para a mesma pergunta saem de sincronia no primeiro que reiniciar.
     *
     * `opacity` e não `hidden`: a transição precisa de algo para animar, e um elemento
     * que sai do fluxo faria o vídeo pular meio segundo antes de o ponteiro parar.
     */
    paintIdle(idle) {
        this.idle = idle;

        const header = document.querySelector('#room > header');
        const caption = this.fullscreen
            && el('stage').querySelector(`[data-screen="${CSS.escape(this.fullscreen)}"] figcaption`);

        for (const element of [header, caption]) {
            element?.classList.toggle('opacity-0', idle);
            // Sem isto a barra invisível continua roubando o clique de quem quer o vídeo.
            element?.classList.toggle('pointer-events-none', idle);
        }

        document.body.classList.toggle('cursor-none', idle);
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
                // Sem tamanho conhecido (Linux sem xdpyinfo) é melhor nada que "0×0".
                detail: display.width ? `${display.width}×${display.height}` : '',
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

        // No Linux o que vai é o monitor da saída de som: o sistema inteiro, sem filtro
        // por aplicativo.
        const note = App.isLinux()
            ? audio && el('mute-calls').checked
                ? 'No Linux vai o som do sistema inteiro: não dá para deixar o Discord de fora.'
                : ''
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
            const reason = App.isLinux()
                ? 'Nenhuma tela X11 encontrada. Em sessão Wayland a captura ainda não funciona.'
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
                // Sempre, mesmo em zero. Um campo que só aparece quando está ruim faz
                // duvidar se ainda funciona — e zero aqui é a informação boa.
                `${stats.sendDropped} perdidos`,
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
        this.nativeWatching.clear();
        await invoke('stop_watch', { peerId: null }).catch(() => null);
        this.remoteAudios.clear();
        this.pausedPeers.clear();
        this.selfStream = null;
        this.focused = null;

        if (this.fullscreen) {
            await this.toggleFullscreen(this.fullscreen);
        }

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
