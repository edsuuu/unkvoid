import { MicrophoneGate } from '@voice/MicrophoneGate.js';
import { SfuClient } from '@voice/SfuClient.js';

import { Api } from './api.js';
import { P2P } from './p2p.js';

const el = id => document.getElementById(id);

// This UI has no bundler, so `window.__TAURI__` (config `withGlobalTauri`) is the only
// bridge to Rust — and the plugin scripts only attach themselves to it once it exists.
// Reading it blind would throw here and leave the update screen spinning forever, which
// is exactly what it looked like before: a hang with no message.
if (! window.__TAURI__?.core) {
    el('update-status').textContent = 'Broken build: the Tauri bridge did not load.';
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
        await this.update();

        el('update-screen').hidden = true;

        if (! await this.serverAnswered()) {
            return;
        }

        this.api.authenticated ? await this.signIn() : this.askForLogin();
    }

    async update() {
        try {
            const version = await invoke('check_update');

            if (version) {
                el('update-status').textContent = `Installing version ${version}…`;
                await invoke('restart');
            }
        } catch (failure) {
            // An update failure must not prevent startup: the app continues on its current version.
            console.warn('update unavailable:', failure);
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
                throw new Error(`the server answered ${response.status}`);
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
        el('offline-attempt').textContent = `attempt ${this.attempt}`;

        clearTimeout(this.reconnect);
        this.reconnect = setTimeout(async () => {
            if (await this.serverAnswered()) {
                el('offline-screen').hidden = true;
                this.attempt = 0;
                this.api.authenticated ? await this.signIn() : this.askForLogin();
            }
        }, Math.min(2000 * this.attempt, 10000));
    }

    askForLogin() {
        el('login-screen').hidden = false;

        // Google does not open inside the app: it opens in the system browser and
        // returns through a deep link. The password never passes through Unkvoid.
        el('google-button').onclick = () => openUrl(`${Api.BASE}/api/desktop/google`);

        onOpenUrl(async ([url]) => {
            const params = new URL(url).searchParams;
            const error = params.get('error');

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
            el('login-screen').hidden = true;
            await this.signIn();
        });

        el('login-form').onsubmit = async event => {
            event.preventDefault();
            el('login-error').textContent = '';
            el('sign-in-button').disabled = true;

            try {
                await this.api.login(el('email').value, el('password').value);
                el('login-screen').hidden = true;
                await this.signIn();
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
                el('my-state').textContent = `could not change autostart: ${failure}`;
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
            el('mic-shortcut').textContent = 'press a key…';

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

            button.className = `server${this.server?.id === server.id ? ' active' : ''}`;
            button.textContent = server.initials;
            button.title = server.name;
            button.onclick = () => this.openServer(server.id);
            rail.appendChild(button);
        }

        const separator = document.createElement('span');

        separator.className = 'separator';
        rail.appendChild(separator);

        const plus = document.createElement('button');

        plus.className = 'server plus';
        plus.textContent = '+';
        plus.title = 'Criar server';
        plus.onclick = () => this.createServer();
        rail.appendChild(plus);
    }

    async createServer() {
        const name = prompt('Server name');

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

            title.className = 'section';
            title.textContent = kind === 'text' ? 'Text channels' : 'Voice channels';
            list.appendChild(title);

            for (const channel of channels) {
                const button = document.createElement('button');

                button.className = `channel${this.channel?.id === channel.id ? ' active' : ''}`;
                button.innerHTML = kind === 'text'
                    ? `<span style="font-size:20px;color:#80848e">#</span><span>${channel.name}</span>`
                    : `<span style="color:#80848e">🔊</span><span>${channel.name}</span><span class="clock" data-clock="${channel.id}"></span>`;
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
        this.drawChannels();

        const messages = await this.api.messages(channel.id);

        el('stage').hidden = true;
        el('empty').hidden = false;
        el('empty').textContent = messages.length
            ? messages.map(message => `${message.author.name}: ${message.content}`).join('\n')
            : 'No messages yet.';
    }

    async joinVoice(channel) {
        el('my-state').textContent = 'connecting…';

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
            el('my-state').textContent = `could not join the room: ${failure.message}`;
            this.p2p = null;
            this.sfu = null;

            return;
        }

        el('voice-bar').hidden = false;
        el('voice-channel').textContent = channel.name;
        el('my-state').textContent = `in ${channel.name}`;
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

        frame.className = 'tile';
        frame.dataset.tela = from;
        frame.innerHTML = '<video autoplay playsinline></video><figcaption></figcaption>';
        frame.querySelector('video').srcObject = stream;
        frame.querySelector('figcaption').textContent = 'broadcasting';

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
            el('my-state').textContent = `microphone unavailable: ${failure.message}`;
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
                audio.dataset.remoto = producerId;
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
            el('my-state').textContent = this.micDenied ? 'the system denied the microphone' : 'join a voice channel first';

            return;
        }

        this.mic.setMuted(! this.mic.muted);
    }

    paintMicrophone({ transmitting, muted, db }) {
        const button = el('mute');

        if (button) {
            button.textContent = muted ? 'Unmute' : 'Mute';
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
        el('my-state').textContent = 'Available';
    }

    async share() {
        if (! this.p2p) {
            el('my-state').textContent = 'join a voice channel first';

            return;
        }

        try {
            await this.p2p.broadcast(el('quality').value, this.participants);
            el('share').hidden = true;
            el('stop').hidden = false;
            el('my-state').textContent = this.participants.length
                ? `broadcasting to ${this.participants.length}`
                : 'broadcasting (no one watching yet)';
        } catch (failure) {
            el('my-state').textContent = failure.message ?? String(failure);
        }
    }

    async stopSharing() {
        const frames = await this.p2p?.stop().catch(() => 0);

        el('share').hidden = false;
        el('stop').hidden = true;

        if (frames) {
            el('my-state').textContent = `${frames} frames broadcast`;
        }
    }
}

new App().start();
