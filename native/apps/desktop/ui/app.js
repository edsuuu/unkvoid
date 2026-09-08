import { MicrophoneGate } from '@voice/MicrophoneGate.js';
import { PresenceClient } from '@voice/PresenceClient.js';
import { SfuClient } from '@voice/SfuClient.js';

import { Api } from './api.js';
import { Broadcast } from './broadcast.js';

const el = id => document.getElementById(id);

const HEAD_ON = '<path d="M12 3a9 9 0 0 0-9 9v5a3 3 0 0 0 3 3h1a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H5v-.5a7 7 0 1 1 14 0v.5h-2a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h1a3 3 0 0 0 3-3v-5a9 9 0 0 0-9-9z"/>';
const HEAD_OFF = '<path d="M4.7 3.3a1 1 0 0 0-1.4 1.4l3 3A8.96 8.96 0 0 0 3 12v5a3 3 0 0 0 3 3h1a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H5v-.5c0-1.3.36-2.5 1-3.53l12.3 12.32a1 1 0 0 0 1.4-1.42L4.7 3.3zM21 12a9 9 0 0 0-13.6-7.75l1.47 1.47A7 7 0 0 1 19 11.5v.5h-2a1 1 0 0 0-1 1v3.17l2 2A3 3 0 0 0 21 17v-5z"/>';

const MIC_ON = '<path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 1 0-6 0v5a3 3 0 0 0 3 3z"/><path d="M18 11a1 1 0 1 0-2 0 4 4 0 0 1-8 0 1 1 0 1 0-2 0 6 6 0 0 0 5 5.917V19H9a1 1 0 1 0 0 2h6a1 1 0 1 0 0-2h-2v-2.083A6 6 0 0 0 18 11z"/>';
const MIC_OFF = '<path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-5.4-1.8l4.2 4.2V11a1 1 0 0 1-1.8.6L12 14zM4.7 3.3a1 1 0 0 0-1.4 1.4l16 16a1 1 0 0 0 1.4-1.4l-3.2-3.2A6 6 0 0 0 18 11a1 1 0 1 0-2 0c0 .7-.18 1.35-.5 1.92l-1.5-1.5V11l-.02.02L9 6.05V6a3 3 0 0 1 .1-.75L4.7 3.3zM6 10a1 1 0 0 0-2 0 6 6 0 0 0 5 5.92V19H9a1 1 0 1 0 0 2h6a1 1 0 0 0 .7-1.71L13 16.58V17h-1a4 4 0 0 1-4-4v-1.17L6.4 10.24A1 1 0 0 0 6 10z"/>';

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
const { listen } = window.__TAURI__.event;
const { openUrl } = window.__TAURI__.opener;
const { onOpenUrl } = window.__TAURI__.deepLink;
const initials = name => (name ?? '?').slice(0, 2).toUpperCase();

/**
 * Segundos em relógio. A hora só aparece depois que ela existe, como no Discord —
 * mas precisa aparecer: sem ela uma call de uma hora virava "64:12".
 */
const clock = seconds => {
    const partes = [Math.floor(seconds / 60) % 60, seconds % 60].map(n => String(n).padStart(2, '0'));

    return seconds >= 3600 ? [Math.floor(seconds / 3600), ...partes].join(':') : partes.join(':');
};

/** Marcador vermelho ao lado do nome de quem está com microfone ou áudio mudo. */
const badge = (mark, icon) => `<span class="hidden shrink-0 items-center text-danger" data-${mark}>`
    + `<svg class="size-4" fill="currentColor" viewBox="0 0 24 24">${icon}</svg></span>`;

class App {
    /** De quanto em quanto tempo procurar versão nova com o app já aberto. */
    static UPDATE_EVERY_MS = 6 * 60 * 60 * 1000;

    constructor() {
        this.api = new Api(() => this.showOffline());

        this.me = 'você';
        this.servers = [];
        this.server = null;
        this.channel = null;

        this.voice = null;
        this.voiceChannel = null;
        this.sfu = null;
        this.broadcast = null;
        this.participants = [];

        // O convite da presença vem por Bearer, e não por sessão do navegador: é a
        // única coisa que o app faz diferente da web aqui.
        this.presence = new PresenceClient(id => this.api.presenceToken(id));
        this.presence.addEventListener('presence', event => this.drawPresence(event.detail));

        this.mic = new MicrophoneGate(state => this.paintMicrophone(state));
        this.micDenied = false;
        this.deafened = false;

        /** Quem está falando agora, por id — para a borda verde sobreviver ao redesenho. */
        this.speaking = new Map();

        /** Função que desliga o medidor de cada áudio remoto. */
        this.watchers = new Map();

        this.shareSource = null;

        /**
         * Transmitindo agora. Mora aqui, e não no mapa de peers do SFU: aquele mapa é
         * reescrito a cada evento da sala, e a flag "ao vivo" apagava sozinha segundos
         * depois de começar a transmissão.
         */
        this.sharing = false;

        /** Último `muted` já contado para a sala — o medidor pinta 20x por segundo. */
        this.mutedShown = null;

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
        this.showDownloadProgress();

        await this.update();

        el('update-screen').hidden = true;

        setInterval(() => void this.update(), App.UPDATE_EVERY_MS);

        if (! await this.serverAnswered()) {
            return;
        }

        await this.enter();
    }

    /**
     * Quanto já baixou, na tela.
     *
     * Sem isto a tela fica parada em "Procurando atualizações…" durante todo o download.
     * O app já chegou às suas mãos assim uma vez, e a leitura correta foi "travou".
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
     * Roda na abertura e de tempos em tempos: o app inicia com o sistema e fica semanas
     * aberto na bandeja, então só olhar na abertura significava esperar o próximo
     * reinício da máquina para ver uma versão publicada hoje.
     */
    async update() {
        try {
            const version = await invoke('check_update');

            if (version) {
                el('update-status').textContent = `Instalando a versão ${version}…`;

                // Reiniciar no meio de uma chamada derruba a pessoa da call. A versão já
                // está instalada no disco; ela passa a valer no próximo reinício.
                if (this.voiceChannel) {
                    el('my-state').textContent = `versão ${version} pronta — reinicie o app`;

                    return;
                }

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

        this.me = eu.name;
        el('my-name').textContent = eu.name;
        el('my-avatar').textContent = initials(eu.name);

        this.servers = await this.api.servers();
        this.drawServerRail();

        el('share').onclick = () => this.openShareModal();
        el('share-cancel').onclick = () => this.closeShareModal();

        el('logout').onclick = async () => {
            await this.leaveVoice();
            this.api.forget();
            this.toggleMicPanel();
            this.servers = [];
            this.server = null;
            this.channel = null;
            el('channel-list').innerHTML = '';
            el('server-rail').innerHTML = '';
            this.showPane('empty');
            this.askForLogin();
        };

        el('share-confirm').onclick = async () => {
            this.closeShareModal();
            await this.share();
        };
        el('stop').onclick = () => this.stopSharing();
        el('leave-voice').onclick = () => this.leaveVoice();
        el('mute').onclick = () => this.toggleMicrophone();
        el('voice-mute').onclick = () => this.toggleMicrophone();
        el('deafen').onclick = () => this.toggleDeafen();
        el('deafen-icon').innerHTML = HEAD_ON;
        el('voice-mute-icon').innerHTML = MIC_ON;
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
        // Iniciar com o sistema é o padrão, não uma opção: um app de voz que só existe
        // depois de alguém lembrar de abrir perde a chamada. Ligamos uma vez e
        // registramos que já foi feito — se a pessoa desligar por fora, fica desligado.
        try {
            if (! await invoke('get_setting', { key: 'autostart:asked' })) {
                await invoke('set_autostart', { enabled: true });
                await invoke('set_setting', { key: 'autostart:asked', value: 'sim' });
            }
        } catch (failure) {
            console.warn('início automático indisponível:', failure);
        }

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
        this.drawThreshold(threshold);
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

        el('mic-threshold').oninput = event => {
            const threshold = Number(event.target.value);

            this.rememberMic({ threshold });
            this.drawThreshold(threshold);
        };
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

    /**
     * Quem está no canal de voz, listado logo abaixo dele.
     *
     * Você entra na lista **antes** do handshake terminar: sem isso, clicar no canal
     * parece não fazer nada durante os segundos de conexão.
     */
    drawParticipants(channelId, people) {
        const lista = document.querySelector(`[data-participants="${channelId}"]`);

        if (! lista) {
            return;
        }

        lista.className = people.length ? 'mb-1 ml-6 space-y-0.5' : '';
        lista.innerHTML = '';

        for (const person of people) {
            const linha = document.createElement('div');

            linha.className = `flex items-center gap-2 rounded px-2 py-1 text-sm ${person.connecting ? 'text-ink-dim italic' : 'text-ink-soft'}`;
            linha.dataset.participant = person.id ?? '';
            linha.innerHTML = '<span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-brand text-[10px] font-semibold text-white ring-2 ring-transparent transition-[box-shadow]" data-avatar></span>'
                + `<span class="truncate" data-name></span><span class="ml-auto"></span>${badge('muted', MIC_OFF)}${badge('deafened', HEAD_OFF)}`
                + '<span class="hidden shrink-0 items-center gap-1 rounded bg-danger px-1.5 py-0.5 text-[10px] font-bold uppercase leading-none text-white" data-live>'
                + '<span class="size-1.5 rounded-full bg-white"></span>ao vivo</span>';

            linha.querySelector('[data-avatar]').textContent = initials(person.name);
            linha.querySelector('[data-name]').textContent = person.connecting
                ? `${person.name} · conectando…`
                : person.name;

            // `hidden` do Tailwind é uma classe, não o atributo: alternar as duas
            // deixaria o elemento visível com display:none.
            for (const [marca, ligado] of [['live', person.sharing], ['muted', person.muted], ['deafened', person.deafened]]) {
                const marcador = linha.querySelector(`[data-${marca}]`);

                marcador.classList.toggle('hidden', ! ligado);
                marcador.classList.toggle('flex', Boolean(ligado));
            }

            lista.appendChild(linha);
        }
    }

    /**
     * Todo mundo que o SFU conhece na sala.
     *
     * A lista do SFU **já inclui você** (marcado com `self`), então acrescentar seu nome
     * por fora faz aparecer duas vezes. Antes de conectar não há lista nenhuma, e aí sim
     * o nome vem daqui.
     */
    refreshParticipants() {
        if (! this.voiceChannel) {
            return;
        }

        const sala = [...(this.sfu?.peers?.entries() ?? [])];

        // O seu estado vem daqui, não do mapa do SFU: aquele mapa é reescrito a cada
        // evento da sala e apagava a sua flag de "ao vivo" sozinho.
        const eu = { muted: this.mic.muted, deafened: this.deafened, sharing: this.sharing };

        this.drawParticipants(
            this.voiceChannel.id,
            sala.length
                ? sala.map(([id, peer]) => ({
                    ...peer,
                    ...(id === this.sfu?.peerId ? eu : {}),
                    id,
                }))
                : [{ name: this.me, ...eu }],
        );

        // Redesenhar apaga as bordas: quem estava falando volta a acender no próximo
        // quadro de áudio, mas quem já estava falando não pode piscar.
        for (const [id, falando] of this.speaking) {
            this.paintSpeaking(id, falando);
        }
    }

    /**
     * Quem está em cada canal de voz, direto do servidor.
     *
     * Vale para **todos** os canais, não só o seu. É por isto que você vê quem está numa
     * conversa antes de entrar nela, e continua vendo quem ficou — mudo, surdo ou
     * transmitindo — depois de sair. A lista da sua própria conexão morre junto com ela;
     * esta não.
     */
    drawPresence(channels) {
        for (const [channelId, presenca] of Object.entries(channels)) {
            this.drawParticipants(channelId, presenca.members.map(pessoa => this.withMyState(pessoa)));
        }

        // Canal que esvaziou não vem na carga. Sem isto a lista velha ficava na tela.
        for (const lista of document.querySelectorAll('[data-participants]')) {
            if (! channels[lista.dataset.participants]) {
                this.drawParticipants(lista.dataset.participants, []);
            }
        }

        for (const [id, falando] of this.speaking) {
            this.paintSpeaking(id, falando);
        }
    }

    /** O seu estado é local e instantâneo; o que volta do servidor chega depois. */
    withMyState(pessoa) {
        const eu = pessoa.peerId === this.sfu?.peerId
            ? { muted: this.mic.muted, deafened: this.deafened, sharing: this.sharing }
            : {};

        return { ...pessoa, ...eu, id: pessoa.peerId };
    }

    /** Borda verde no avatar de quem está falando, como no Discord. */
    paintSpeaking(peerId, falando) {
        this.speaking.set(peerId, falando);

        const avatar = document.querySelector(`[data-participant="${peerId}"] [data-avatar]`);

        if (avatar) {
            avatar.classList.toggle('ring-online', falando);
            avatar.classList.toggle('ring-transparent', ! falando);
        }
    }

    /** A marca do limiar e o medidor dividem a mesma escala — senão a marca mente. */
    drawThreshold(threshold) {
        const mark = document.querySelector('[data-mark]');

        if (mark) {
            mark.style.left = `${MicrophoneGate.toFraction(threshold) * 100}%`;
        }
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

        // Quem está em cada canal de voz vem do servidor, não da sua própria conexão
        // com a sala: é isto que faz você continuar vendo quem ficou depois de sair da
        // chamada — e ver quem está lá antes mesmo de entrar.
        void this.presence.watch(id).catch(() => {});

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

    /**
     * O miolo mostra um painel de cada vez.
     *
     * Os três dividem o mesmo `flex-1`: deixar dois visíveis não empilha, espreme — era
     * por isso que entrar na chamada jogava as mensagens para o meio da tela.
     */
    showPane(name) {
        for (const pane of ['chat', 'stage', 'empty']) {
            el(pane).hidden = pane !== name;
        }
    }

    async openChannel(channel) {
        this.channel = channel;
        el('channel-title').textContent = `# ${channel.name}`;
        el('draft').placeholder = `Conversar em #${channel.name}`;
        this.drawChannels();
        this.refreshParticipants();

        this.showPane('chat');

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
            // Agrupa falas seguidas da mesma pessoa dentro de 5 minutos: repetir nome e
            // horário a cada frase vira ruído, e um intervalo grande deixa de ser a
            // mesma conversa.
            const seguida = anterior?.author.id === message.author.id
                && new Date(message.created_at) - new Date(anterior.created_at) < 5 * 60_000;

            const linha = document.createElement('div');

            linha.className = `flex gap-4 px-4 hover:bg-black/10 ${seguida ? 'py-0.5' : 'mt-4 py-0.5 first:mt-0'}`;
            linha.innerHTML = seguida
                ? '<span class="w-10 shrink-0"></span><p class="min-w-0 flex-1 break-words leading-relaxed text-ink"></p>'
                : '<span class="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-full bg-brand text-sm font-semibold text-white"></span>'
                    + '<div class="min-w-0 flex-1">'
                    + '<p class="flex items-baseline gap-2 leading-tight">'
                    + '<span class="font-medium text-white"></span>'
                    + '<span class="text-xs text-ink-soft"></span>'
                    + '</p>'
                    + '<p class="break-words leading-relaxed text-ink"></p>'
                    + '</div>';

            // textContent e nunca innerHTML: mensagem é texto de outra pessoa, e montar
            // HTML com ela deixaria qualquer um executar script na tela dos outros.
            if (seguida) {
                linha.querySelector('p').textContent = message.content;
            } else {
                const spans = linha.querySelectorAll('span');
                const paragrafos = linha.querySelectorAll('p');

                spans[0].textContent = initials(message.author.name);
                spans[1].textContent = message.author.name;
                spans[2].textContent = new Date(message.created_at).toLocaleString('pt-BR', {
                    day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit',
                });
                paragrafos[1].textContent = message.content;
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

        this.voiceChannel = channel;
        this.drawParticipants(channel.id, [{ name: this.me, connecting: true }]);

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
            // A lista embaixo do canal acompanha quem entra e sai, sem F5.
            this.sfu.addEventListener('peersChanged', () => this.refreshParticipants());

            this.broadcast = new Broadcast(this.sfu);

            const joined = await this.sfu.connect(
                this.voice.url,
                async () => (await this.api.voiceToken(channel.id)).token,
            );


            this.participants = joined.peers.map(peer => peer.peerId);

            for (const peer of joined.peers) {
                for (const producer of peer.producers) {
                    await this.consume({ ...producer, peerId: peer.peerId, name: peer.name });
                }
            }

            await this.openMicrophone();
        } catch (failure) {
            el('my-state').textContent = `não deu para entrar na sala: ${failure.message}`;
            this.broadcast = null;
            this.sfu = null;

            return;
        }

        el('voice-bar').hidden = false;
        el('voice-channel').textContent = channel.name;
        el('my-state').textContent = `em ${channel.name}`;
        this.refreshParticipants();

        // Estar em chamada não tira o texto da frente: o palco só toma a tela quando
        // alguém transmite de fato, senão a conversa dá lugar a um vazio.
        this.showPane(this.channel ? 'chat' : 'stage');

        const startedAt = Date.now();

        clearInterval(this.clock);
        this.clock = setInterval(() => {
            const seconds = Math.floor((Date.now() - startedAt) / 1000);
            const label = clock(seconds);

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

            if (! el('stage').childElementCount) {
                this.showPane(this.channel ? 'chat' : 'empty');
            }

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

        this.showPane('stage');
    }

    /**
     * Voice goes through the SFU: audio is cheap and the server already fans it
     * out to everyone in the room, so talking works with any number of people — while the
     * screen, which is expensive, stays direct between machines.
     */
    async openMicrophone() {
        try {
            const micTrack = await this.mic.open();

            await this.sfu.publishMicrophone(micTrack);

            // Entra mudo. Abrir o microfone junto com a chamada joga na sala o que
            // estava acontecendo no quarto de quem entrou, antes de ela perceber.
            this.mic.setMuted(true);
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
                audio.muted = this.deafened;
                document.body.appendChild(audio);

                const stream = audio.srcObject;

                this.watchers.set(
                    producerId,
                    await MicrophoneGate.watch(stream, falando => this.paintSpeaking(peerId, falando)),
                );

                return;
            }

            // Toda transmissão publica no SFU, do app ou da web: é assim que o desktop
            // watches someone who is not using the app.
            this.showScreen(peerId, new MediaStream([consumer.track]));
        } catch (failure) {
            console.warn('could not receive media:', failure);
        }
    }

    /**
     * Ensurdecer: cala o áudio da sala inteira sem sair dela.
     *
     * Silencia o próprio microfone junto, como no Discord — quem não está ouvindo não
     * deveria continuar falando sem perceber.
     */
    toggleDeafen() {
        this.deafened = ! this.deafened;

        for (const audio of document.querySelectorAll('audio[data-remote]')) {
            audio.muted = this.deafened;
        }

        if (this.deafened && ! this.mic.muted) {
            this.mic.setMuted(true);
        }

        el('deafen-icon').innerHTML = this.deafened ? HEAD_OFF : HEAD_ON;
        el('deafen').classList.toggle('text-danger', this.deafened);
        el('deafen').title = this.deafened ? 'Ouvir a sala de novo' : 'Silenciar o áudio da sala';

        this.sfu?.reportState(this.mic.muted, this.deafened);
        this.refreshParticipants();
    }

    /** Mute é o portão, nunca o producer: a chamada mantém o caminho do áudio quente. */
    toggleMicrophone() {
        if (! this.mic.active) {
            el('my-state').textContent = this.micDenied ? 'o sistema negou o microfone' : 'entre num canal de voz primeiro';

            return;
        }

        this.mic.setMuted(! this.mic.muted);
    }

    paintMicrophone({ transmitting, muted, db }) {
        // O mesmo estado pinta os dois botões: o da barra de voz e o do rodapé.
        for (const id of ['mute', 'voice-mute']) {
            const botao = el(id);

            if (! botao) {
                continue;
            }

            el(`${id}-icon`).innerHTML = muted ? MIC_OFF : MIC_ON;
            botao.title = muted ? 'Ativar o microfone' : 'Silenciar microfone';
            botao.classList.toggle('text-danger', muted);
            botao.classList.toggle('text-online', ! muted && transmitting);
            botao.classList.toggle('text-ink', ! muted && ! transmitting);
        }

        if (this.sfu?.peerId) {
            this.paintSpeaking(this.sfu.peerId, ! muted && transmitting);
        }

        // Só quando muda de verdade: isto aqui roda a cada quadro de áudio, vinte vezes
        // por segundo, e redesenhar a sala nesse ritmo é um piscar constante.
        if (muted !== this.mutedShown) {
            this.mutedShown = muted;
            this.sfu?.reportState(muted, this.deafened);
            this.refreshParticipants();
        }

        const meter = document.querySelector('[data-meter]');

        if (meter) {
            meter.style.width = `${MicrophoneGate.toFraction(db) * 100}%`;
            meter.style.background = transmitting ? '#23a55a' : '#4e5058';
        }
    }

    async leaveVoice() {
        for (const parar of this.watchers.values()) {
            parar();
        }

        this.watchers.clear();
        this.speaking.clear();

        if (this.voiceChannel) {
            this.voiceChannelId = this.voiceChannel.id;
            this.drawParticipants(this.voiceChannel.id, []);
            this.voiceChannel = null;
        }

        clearInterval(this.clock);

        if (this.voiceChannelId) {
            const relogio = document.querySelector(`[data-clock="${this.voiceChannelId}"]`);

            if (relogio) {
                relogio.textContent = '';
            }
        }

        this.voiceChannelId = null;
        await this.stopSharing();
        this.mic.close();
        this.micDenied = false;
        this.mutedShown = null;
        await this.sfu?.leaveRoom();
        this.sfu?.disconnect();
        this.broadcast = null;
        this.sfu = null;
        this.voice = null;
        document.querySelectorAll('audio[data-remote]').forEach(elemento => elemento.remove());
        el('voice-bar').hidden = true;
        el('stage').innerHTML = '';
        this.showPane(this.channel ? 'chat' : 'empty');
        el('my-state').textContent = 'Disponível';
    }

    /**
     * Abre a escolha do que transmitir.
     *
     * A lista vem do sistema operacional, não de um palpite: `list_displays` e
     * `list_windows` são os mesmos que o macOS usa para montar o seletor dele.
     */
    async openShareModal() {
        if (! this.sfu) {
            el('my-state').textContent = 'entre num canal de voz primeiro';

            return;
        }

        this.shareSource = null;
        el('share-confirm').disabled = true;
        el('share-modal').hidden = false;

        for (const aba of document.querySelectorAll('[data-tab]')) {
            aba.onclick = () => this.drawShareTab(aba.dataset.tab);
        }

        // A lista vem do sistema operacional, não de um palpite: são os mesmos dados
        // que o macOS usa para montar o seletor dele.
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

        lista.className = 'mt-4 grid min-h-0 flex-1 auto-rows-min grid-cols-2 gap-3 overflow-y-auto content-start';

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
            outro.classList.toggle('bg-brand', escolhido);
        }

        this.shareSource = botao.dataset.source;
        el('share-confirm').disabled = false;
    }

    closeShareModal() {
        el('share-modal').hidden = true;
    }

    async share() {
        if (! this.sfu) {
            el('my-state').textContent = 'entre num canal de voz primeiro';

            return;
        }

        try {
            await this.broadcast.start(el('quality').value, this.shareSource);

            this.paintSharing(true);
            el('my-state').textContent = this.participants.length
                ? `transmitindo para ${this.participants.length}`
                : 'transmitindo (ninguém assistindo ainda)';
        } catch (failure) {
            // Falhar calado deixava a barra sem botão nenhum: quem tentou compartilhar
            // via o modal fechar e mais nada.
            this.paintSharing(false);
            el('my-state').textContent = `não deu para transmitir: ${failure.message ?? failure}`;
        }
    }

    /** Barra de voz e lista da sala concordando sobre você estar ao vivo ou não. */
    paintSharing(on) {
        this.sharing = on;

        el('share').hidden = on;
        el('stop').hidden = ! on;
        el('live-note').hidden = ! on;

        this.refreshParticipants();
    }

    async stopSharing() {
        const frames = await this.broadcast?.stop().catch(() => 0);

        this.paintSharing(false);

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
