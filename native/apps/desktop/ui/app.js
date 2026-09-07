import { MicrophoneGate } from '@voice/MicrophoneGate.js';
import { SfuClient } from '@voice/SfuClient.js';

import { Api } from './api.js';
import { P2P } from './p2p.js';

const el = id => document.getElementById(id);

/** Aparência dos elementos que o JavaScript cria. */
const LOOK = {
    server: 'group relative flex size-12 cursor-pointer items-center justify-center rounded-[24px] text-sm font-semibold transition-all duration-200 hover:rounded-2xl hover:bg-brand hover:text-white',
    serverIdle: 'bg-content text-ink',
    serverActive: 'rounded-2xl bg-brand text-white',
    plus: 'flex size-12 cursor-pointer items-center justify-center rounded-[24px] bg-content text-2xl leading-none text-online transition-all duration-200 hover:rounded-2xl hover:bg-online hover:text-white',
    separator: 'h-0.5 w-8 shrink-0 rounded-full bg-line',
    section: 'px-2 pt-4 pb-1 text-xs font-bold uppercase tracking-wide text-ink-soft',
    channel: 'flex w-full cursor-pointer items-center gap-1.5 rounded px-2 py-1.5 text-left text-[15px] transition-colors duration-100',
    channelIdle: 'text-ink-soft hover:bg-line hover:text-ink',
    channelActive: 'bg-[#404249] text-white',
    tile: 'm-0 flex flex-col overflow-hidden rounded-lg bg-black',
};

// This UI has no bundler, so `window.__TAURI__` (config `withGlobalTauri`) is the only
// bridge to Rust — and the plugin scripts only attach themselves to it once it exists.
// Reading it blind would throw here and leave the update screen spinning forever, which
// is exactly what it looked like before: a hang with no message.
if (! window.__TAURI__?.core) {
    el('update-status').textContent = 'Build quebrada: a ponte do Tauri não carregou.';
    throw new Error('window.__TAURI__ is missing — check withGlobalTauri in tauri.conf.json');
}

const { invoke } = window.__TAURI__.core;
const { openUrl } = window.__TAURI__.opener;
const { onOpenUrl } = window.__TAURI__.deepLink;
const initials = name => (name ?? '?').slice(0, 2).toUpperCase();

class App {
    constructor() {
        this.api = new Api(() => this.showOffline());
        this.servers = [];
        this.server = null;
        this.channel = null;
        this.voice = null;
        this.sfu = null;
        this.p2p = null;
        this.mic = new MicrophoneGate(state => this.paintMicrophone(state));
        this.micPanel = null;
        this.participants = [];
        this.clock = null;
        this.attempt = 0;
    }

    /**
     * Startup order, like Discord: update first, require the server next, and only
     * then ask for login. Entering an outdated app or one without a server would
     * only produce an error later.
     */
    async start() {
        // Antes de tudo, e uma vez só: o link pode chegar com o app em qualquer tela —
        // offline, login, já dentro. Registrar isto dentro da tela de login fazia o
        // token se perder em qualquer outro estado.
        this.listenForDeepLink();

        await this.update();

        el('update-screen').hidden = true;

        if (! await this.serverAnswered()) {
            return;
        }

        await this.enter();
    }

    async update() {
        try {
            const version = await invoke('check_update');

            if (version) {
                el('update-status').textContent = `Instalando a versão ${version}…`;
                await invoke('restart');
            }
        } catch (failure) {
            // Falhar a atualização não pode impedir o app de abrir: ele continua na
            // versão atual.
            console.warn('atualização indisponível:', failure);
        }
    }

    /**
     * `/api/health` e não `/api/me`: a segunda exige token, responde 302 para /login sem
     * ele, e a página de login não tem cabeçalho CORS — o fetch segue o redirecionamento,
     * rejeita, e o app conclui que o servidor caiu. Um app recém-instalado, que ainda não
     * tem token, nunca passava dessa tela.
     */
    async serverAnswered() {
        try {
            const response = await fetch(`${Api.BASE}/api/health`, { method: 'GET' });

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
        el('login-screen').hidden = true;

        this.attempt += 1;
        el('offline-attempt').textContent = `tentativa ${this.attempt}`;

        clearTimeout(this.reconnect);
        this.reconnect = setTimeout(async () => {
            if (await this.serverAnswered()) {
                el('offline-screen').hidden = true;
                this.attempt = 0;
                await this.enter();
            }
        }, Math.min(2000 * this.attempt, 10000));
    }

    /** Entra se já houver token; senão pede login. */
    async enter() {
        if (! this.api.authenticated) {
            this.askForLogin();

            return;
        }

        try {
            el('login-screen').hidden = true;
            await this.signIn();
        } catch (failure) {
            // Sem isto, uma falha aqui esconde a tela de login e não mostra nada no
            // lugar: o app fica numa tela morta sem explicar o motivo.
            this.api.token = null;
            this.askForLogin();
            el('login-error').textContent = `não deu para entrar: ${failure.message ?? failure}`;
        }
    }

    listenForDeepLink() {
        onOpenUrl(async ([url]) => {
            const params = new URL(url).searchParams;

            // `erro`, não `error`: é o nome que o servidor manda no deep link. Trocar um
            // pelo outro faz a falha chegar e ser ignorada — a tela de login fica muda e
            // parece que o botão não fez nada.
            const error = params.get('erro');

            if (error) {
                el('login-error').textContent = error;

                return;
            }

            const token = params.get('token');

            if (! token) {
                return;
            }

            localStorage.setItem('api:token', token);
            this.api.token = token;

            // A tela de atualização já passou nesta instância; o login não pode
            // trazê-la de volta.
            el('update-screen').hidden = true;
            await this.enter();
        });
    }

    askForLogin() {
        el('login-screen').hidden = false;

        // O Google não abre dentro do app: abre no navegador do sistema e volta por
        // deep link. A senha nunca passa pela janela do Unkvoid.
        //
        // O catch não é decoração: o plugin `opener` recusa URL fora do escopo e a
        // promessa rejeita sem nada aparecer. Sem isto, um erro de permissão vira um
        // botão que não faz nada — e foi exatamente o que aconteceu.
        el('google-button').onclick = async () => {
            el('login-error').textContent = '';

            try {
                await openUrl(`${Api.BASE}/api/desktop/google`);
            } catch (failure) {
                el('login-error').textContent = `não deu para abrir o navegador: ${failure}`;
            }
        };

        el('login-form').onsubmit = async event => {
            event.preventDefault();
            el('login-error').textContent = '';
            el('sign-in-button').disabled = true;

            try {
                await this.api.login(el('email').value, el('password').value);
                await this.enter();
            } catch (failure) {
                el('login-error').textContent = failure.message;
            } finally {
                el('sign-in-button').disabled = false;
            }
        };
    }

    async signIn() {
        const eu = await this.api.me();

        el('my-name').textContent = eu.name;
        el('my-avatar').textContent = initials(eu.name);

        this.servers = await this.api.servers();
        this.drawServerRail();

        el('share').onclick = () => this.share();
        el('stop').onclick = () => this.stopSharing();
        el('leave-voice').onclick = () => this.leaveVoice();
        el('mute').onclick = () => this.toggleMicrophone();
        el('mic-settings').onclick = () => this.toggleMicPanel();

        el('message-form').onsubmit = async event => {
            event.preventDefault();

            const texto = el('draft').value;

            el('draft').value = '';
            await this.sendMessage(texto);
        };
        this.wireMicPanel();
        void this.wireMachineSettings();
    }

    /**
     * The panel is static markup wired once. The settings live in the gate, which is
     * what actually decides frame by frame whether the audio leaves this machine.
     */
    /**
     * O que é desta máquina fica nesta máquina.
     *
     * Bind de teclado e iniciar com o sistema não fazem sentido viajando entre
     * computadores — o servidor guarda quem é a pessoa, o SQLite guarda como este
     * computador se comporta.
     */
    async wireMachineSettings() {
        const autostart = el('autostart');

        autostart.checked = await invoke('autostart_enabled').catch(() => false);

        autostart.onchange = async () => {
            try {
                await invoke('set_autostart', { enabled: autostart.checked });
                await invoke('set_setting', { key: 'autostart', value: String(autostart.checked) });
            } catch (failure) {
                // Reverter o visual: um checkbox marcado que não valeu mente para quem clicou.
                autostart.checked = ! autostart.checked;
                el('my-state').textContent = `não deu para mudar o início automático: ${failure}`;
            }
        };

        // O gate guarda as preferências no localStorage do webview, que some se o app
        // for reinstalado. O banco é a cópia que sobrevive.
        for (const [key, value] of await invoke('all_settings').catch(() => [])) {
            if (key.startsWith('mic:')) {
                this.mic.save({ [key.slice(4)]: JSON.parse(value) });
            }
        }
    }

    wireMicPanel() {
        const { mode, threshold, pushKey, noiseSuppression } = this.mic.settings;

        el('mic-threshold').value = threshold;
        el('mic-noise').checked = noiseSuppression;
        el('mic-shortcut').textContent = pushKey;

        for (const option of document.querySelectorAll('input[name="mic-mode"]')) {
            option.checked = option.value === mode;
            option.onchange = () => {
                this.rememberMic({ mode: option.value });
                el('mic-voice').hidden = option.value !== 'voice';
                el('mic-key').hidden = option.value !== 'ptt';
            };
        }

        el('mic-voice').hidden = mode !== 'voice';
        el('mic-key').hidden = mode !== 'ptt';

        el('mic-threshold').oninput = event => this.rememberMic({ threshold: Number(event.target.value) });
        el('mic-noise').onchange = event => this.rememberMic({ noiseSuppression: event.target.checked });

        el('mic-shortcut').onclick = () => {
            el('mic-shortcut').textContent = 'pressione uma tecla…';

            // `once` matters: without it every later keypress would keep rebinding.
            window.addEventListener('keydown', event => {
                event.preventDefault();
                this.rememberMic({ pushKey: event.code });
                el('mic-shortcut').textContent = event.code;
            }, { once: true, capture: true });
        };
    }

    /** Aplica no gate e guarda no banco, para sobreviver a uma reinstalação. */
    rememberMic(changes) {
        this.mic.save(changes);

        for (const [key, value] of Object.entries(changes)) {
            void invoke('set_setting', { key: `mic:${key}`, value: JSON.stringify(value) }).catch(() => {});
        }
    }

    toggleMicPanel() {
        el('mic-panel').hidden = ! el('mic-panel').hidden;
    }

    drawServerRail() {
        const rail = el('server-rail');

        rail.innerHTML = '';

        for (const server of this.servers) {
            const button = document.createElement('button');

            button.className = `${LOOK.server} ${this.server?.id === server.id ? LOOK.serverActive : LOOK.serverIdle}`;
            button.textContent = server.initials;
            button.title = server.name;
            button.onclick = () => this.openServer(server.id);
            rail.appendChild(button);
        }

        const separator = document.createElement('span');

        separator.className = LOOK.separator;
        rail.appendChild(separator);

        const plus = document.createElement('button');

        plus.className = LOOK.plus;
        plus.textContent = '+';
        plus.title = 'Criar servidor';
        plus.onclick = () => this.createServer();
        rail.appendChild(plus);
    }

    async createServer() {
        const name = prompt('Nome do servidor');

        if (! name?.trim()) {
            return;
        }

        const server = await this.api.createServer(name.trim());

        this.servers = await this.api.servers();
        await this.openServer(server.id);
    }

    async openServer(id) {
        this.server = await this.api.server(id);

        el('server-name').textContent = this.server.name;
        el('empty').hidden = true;
        this.drawServerRail();
        this.drawChannels();

        const text = this.server.channels.find(channel => channel.type === 'text');

        if (text) {
            await this.openChannel(text);
        }
    }

    drawChannels() {
        const list = el('channel-list');

        list.innerHTML = '';

        for (const kind of ['text', 'voice']) {
            const channels = this.server.channels.filter(channel => channel.type === kind);

            if (! channels.length) {
                continue;
            }

            const title = document.createElement('p');

            title.className = LOOK.section;
            title.textContent = kind === 'text' ? 'Canais de texto' : 'Canais de voz';
            list.appendChild(title);

            for (const channel of channels) {
                const button = document.createElement('button');

                button.className = `${LOOK.channel} ${this.channel?.id === channel.id ? LOOK.channelActive : LOOK.channelIdle}`;
                button.innerHTML = kind === 'text'
                    ? `<span class="text-xl leading-none text-ink-dim">#</span><span>${channel.name}</span>`
                    : `<span class="text-ink-dim">🔊</span><span>${channel.name}</span><span class="ml-auto shrink-0 font-mono text-xs text-online" data-clock="${channel.id}"></span>`;
                button.onclick = () => (kind === 'text' ? this.openChannel(channel) : this.joinVoice(channel));
                list.appendChild(button);

                if (kind === 'voice') {
                    const members = document.createElement('div');

                    members.dataset.participants = channel.id;
                    list.appendChild(members);
                }
            }
        }
    }

    async openChannel(channel) {
        this.channel = channel;
        el('channel-title').textContent = `# ${channel.name}`;
        el('draft').placeholder = `Conversar em #${channel.name}`;
        this.drawChannels();

        el('stage').hidden = true;
        el('empty').hidden = true;
        el('chat').hidden = false;

        this.drawMessages(await this.api.messages(channel.id));
    }

    /**
     * Uma linha por mensagem, agrupando falas seguidas da mesma pessoa como no Discord:
     * repetir nome e horário a cada frase vira ruído numa conversa rápida.
     */
    drawMessages(messages) {
        const lista = el('messages');

        lista.innerHTML = '';

        if (! messages.length) {
            lista.innerHTML = '<p class="pt-8 text-center text-sm text-ink-soft">Nenhuma mensagem ainda. Manda a primeira.</p>';

            return;
        }

        let anterior = null;

        for (const message of messages) {
            const mesmaPessoa = anterior?.author.id === message.author.id;
            const linha = document.createElement('div');

            linha.className = mesmaPessoa ? 'flex gap-3' : 'flex gap-3 pt-2';
            linha.innerHTML = mesmaPessoa
                ? `<span class="w-10 shrink-0"></span>
                   <p class="min-w-0 break-words text-ink"></p>`
                : `<span class="flex size-10 shrink-0 items-center justify-center rounded-full bg-brand text-xs font-semibold text-white">${initials(message.author.name)}</span>
                   <div class="min-w-0 flex-1">
                     <p class="mb-0.5 flex items-baseline gap-2">
                       <span class="font-medium text-white"></span>
                       <span class="text-xs text-ink-soft"></span>
                     </p>
                     <p class="break-words text-ink"></p>
                   </div>`;

            // textContent e não innerHTML: mensagem é texto de outra pessoa, e montar
            // HTML com ela deixaria qualquer um executar script na tela dos outros.
            const partes = linha.querySelectorAll('p, span');

            if (mesmaPessoa) {
                linha.querySelector('p').textContent = message.content;
            } else {
                partes[1].textContent = message.author.name;
                partes[2].textContent = new Date(message.created_at).toLocaleString('pt-BR', {
                    day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit',
                });
                linha.querySelectorAll('p')[1].textContent = message.content;
            }

            lista.appendChild(linha);
            anterior = message;
        }

        lista.scrollTop = lista.scrollHeight;
    }

    async sendMessage(content) {
        if (! content.trim() || ! this.channel) {
            return;
        }

        try {
            await this.api.sendMessage(this.channel.id, content.trim());
            this.drawMessages(await this.api.messages(this.channel.id));
        } catch (failure) {
            el('my-state').textContent = `não deu para enviar: ${failure.message}`;
        }
    }

    async joinVoice(channel) {
        el('my-state').textContent = 'conectando…';

        try {
            this.voice = await this.api.voiceToken(channel.id);
        } catch (failure) {
            el('my-state').textContent = failure.message;

            return;
        }

        try {
            // One client for everything: voice rides the SFU (which fans out to any
            // number of people), while the screen stays direct between machines.
            this.sfu = new SfuClient();
            this.sfu.addEventListener('newProducer', event => this.consume(event.detail));

            this.p2p = new P2P(this.sfu, (from, stream) => this.showScreen(from, stream));

            const joined = await this.sfu.connect(
                this.voice.url,
                async () => (await this.api.voiceToken(channel.id)).token,
            );

            await this.p2p.attach();

            this.participants = joined.peers.map(peer => peer.peerId);

            for (const peer of joined.peers) {
                for (const producer of peer.producers) {
                    await this.consume({ ...producer, peerId: peer.peerId, name: peer.name });
                }
            }

            await this.openMicrophone();
        } catch (failure) {
            el('my-state').textContent = `não deu para entrar na sala: ${failure.message}`;
            this.p2p = null;
            this.sfu = null;

            return;
        }

        el('voice-bar').hidden = false;
        el('voice-channel').textContent = channel.name;
        el('my-state').textContent = `em ${channel.name}`;
        el('stage').hidden = false;
        el('empty').hidden = true;

        const startedAt = Date.now();

        clearInterval(this.clock);
        this.clock = setInterval(() => {
            const seconds = Math.floor((Date.now() - startedAt) / 1000);
            const label = `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;

            el('voice-clock').textContent = label;

            const channelClock = document.querySelector(`[data-clock="${channel.id}"]`);

            if (channelClock) {
                channelClock.textContent = label;
            }
        }, 1000);
    }

    /** Draws (or removes) the screen of someone who is broadcasting. */
    showScreen(from, stream) {
        const existing = document.querySelector(`[data-screen="${from}"]`);

        if (! stream) {
            existing?.remove();

            return;
        }

        const frame = existing ?? document.createElement('figure');

        frame.className = LOOK.tile;
        frame.dataset.screen = from;
        frame.innerHTML = '<video class="min-h-0 w-full flex-1 object-contain" autoplay playsinline></video>'
            + '<figcaption class="bg-panel px-3 py-1.5 text-xs text-ink"></figcaption>';
        frame.querySelector('video').srcObject = stream;
        frame.querySelector('figcaption').textContent = 'transmitindo';

        if (! existing) {
            el('stage').appendChild(frame);
        }

        const total = el('stage').childElementCount;

        el('stage').style.gridTemplateColumns = `repeat(${total > 1 ? 2 : 1}, minmax(0, 1fr))`;
    }

    /**
     * Voice goes through the SFU, not P2P: audio is cheap and the server already fans it
     * out to everyone in the room, so talking works with any number of people — while the
     * screen, which is expensive, stays direct between machines.
     */
    async openMicrophone() {
        try {
            const micTrack = await this.mic.open();

            await this.sfu.publishMicrophone(micTrack);
            this.paintMicrophone({ db: MicrophoneGate.FLOOR_DB, transmitting: false, muted: false });
        } catch (failure) {
            this.micDenied = true;
            el('my-state').textContent = `microfone indisponível: ${failure.message}`;
        }
    }

    /** Someone else's audio or screen arriving through the SFU. */
    async consume({ producerId }) {
        try {
            const { consumer, peerId } = await this.sfu.consume(producerId);

            if (consumer.kind === 'audio') {
                const audio = document.createElement('audio');

                audio.srcObject = new MediaStream([consumer.track]);
                audio.autoplay = true;
                audio.dataset.remote = producerId;
                document.body.appendChild(audio);

                return;
            }

            // A web broadcaster publishes to the SFU, not P2P: this is how the desktop
            // watches someone who is not using the app.
            this.showScreen(peerId, new MediaStream([consumer.track]));
        } catch (failure) {
            console.warn('could not receive media:', failure);
        }
    }

    /** Mute is the gate, never the producer: the call keeps the audio path warm. */
    toggleMicrophone() {
        if (! this.mic.active) {
            el('my-state').textContent = this.micDenied ? 'o sistema negou o microfone' : 'entre num canal de voz primeiro';

            return;
        }

        this.mic.setMuted(! this.mic.muted);
    }

    paintMicrophone({ transmitting, muted, db }) {
        const button = el('mute');

        if (button) {
            button.textContent = muted ? 'Ativar som' : 'Silenciar';
            button.classList.toggle('danger', muted);
        }

        const meter = document.querySelector('[data-meter]');

        if (meter) {
            meter.style.width = `${MicrophoneGate.toFraction(db) * 100}%`;
            meter.style.background = transmitting ? '#23a55a' : '#4e5058';
        }
    }

    async leaveVoice() {
        clearInterval(this.clock);
        await this.stopSharing();
        this.mic.close();
        this.micDenied = false;
        await this.sfu?.leaveRoom();
        this.sfu?.disconnect();
        this.p2p?.close();
        this.p2p = null;
        this.sfu = null;
        this.voice = null;
        document.querySelectorAll('audio[data-remote]').forEach(elemento => elemento.remove());
        el('voice-bar').hidden = true;
        el('stage').hidden = true;
        el('empty').hidden = false;
        el('my-state').textContent = 'Disponível';
    }

    async share() {
        if (! this.p2p) {
            el('my-state').textContent = 'entre num canal de voz primeiro';

            return;
        }

        try {
            await this.p2p.broadcast(el('quality').value, this.participants);
            el('share').hidden = true;
            el('stop').hidden = false;
            el('my-state').textContent = this.participants.length
                ? `transmitindo para ${this.participants.length}`
                : 'transmitindo (ninguém assistindo ainda)';
        } catch (failure) {
            el('my-state').textContent = failure.message ?? String(failure);
        }
    }

    async stopSharing() {
        const frames = await this.p2p?.stop().catch(() => 0);

        el('share').hidden = false;
        el('stop').hidden = true;

        if (frames) {
            el('my-state').textContent = `${frames} quadros transmitidos`;
        }
    }
}

const app = new App();

// Exposto de propósito: a janela do app não tem console, e quando algo dá errado em
// produção esta é a única forma de inspecionar o estado sem recompilar.
window.unkvoid = app;

app.start();
