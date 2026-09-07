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
const iniciais = nome => (nome ?? '?').slice(0, 2).toUpperCase();

class App {
    constructor() {
        this.api = new Api(() => this.mostrarOffline());
        this.servidores = [];
        this.servidor = null;
        this.canal = null;
        this.voz = null;
        this.sfu = null;
        this.p2p = null;
        this.mic = new MicrophoneGate(estado => this.pintarMicrofone(estado));
        this.painelMic = null;
        this.participantes = [];
        this.relogio = null;
        this.tentativa = 0;
    }

    /**
     * Startup order, like Discord: update first, require the server next, and only
     * then ask for login. Entering an outdated app or one without a server would
     * only produce an error later.
     */
    async start() {
        await this.atualizar();

        el('tela-update').hidden = true;

        if (! await this.servidorRespondeu()) {
            return;
        }

        this.api.authenticated ? await this.entrar() : this.pedirLogin();
    }

    async atualizar() {
        try {
            const versao = await invoke('check_update');

            if (versao) {
                el('update-status').textContent = `Installing version ${versao}…`;
                await invoke('restart');
            }
        } catch (falha) {
            // An update failure must not prevent startup: the app continues on its current version.
            console.warn('update unavailable:', falha);
        }
    }

    async servidorRespondeu() {
        try {
            await fetch(`${Api.BASE}/api/me`, { method: 'GET' });

            return true;
        } catch {
            this.mostrarOffline();

            return false;
        }
    }

    mostrarOffline() {
        el('tela-offline').hidden = false;
        el('tela-login').hidden = true;

        this.tentativa += 1;
        el('offline-tentativa').textContent = `attempt ${this.tentativa}`;

        clearTimeout(this.reconectar);
        this.reconectar = setTimeout(async () => {
            if (await this.servidorRespondeu()) {
                el('tela-offline').hidden = true;
                this.tentativa = 0;
                this.api.authenticated ? await this.entrar() : this.pedirLogin();
            }
        }, Math.min(2000 * this.tentativa, 10000));
    }

    pedirLogin() {
        el('tela-login').hidden = false;

        // Google does not open inside the app: it opens in the system browser and
        // returns through a deep link. The password never passes through Unkvoid.
        el('botao-google').onclick = () => openUrl(`${Api.BASE}/api/desktop/google`);

        onOpenUrl(async ([url]) => {
            const parametros = new URL(url).searchParams;
            const erro = parametros.get('erro');

            if (erro) {
                el('erro-login').textContent = erro;

                return;
            }

            const token = parametros.get('token');

            if (! token) {
                return;
            }

            localStorage.setItem('api:token', token);
            this.api.token = token;
            el('tela-login').hidden = true;
            await this.entrar();
        });

        el('form-login').onsubmit = async evento => {
            evento.preventDefault();
            el('erro-login').textContent = '';
            el('botao-entrar').disabled = true;

            try {
                await this.api.login(el('email').value, el('senha').value);
                el('tela-login').hidden = true;
                await this.entrar();
            } catch (falha) {
                el('erro-login').textContent = falha.message;
            } finally {
                el('botao-entrar').disabled = false;
            }
        };
    }

    async entrar() {
        const eu = await this.api.me();

        el('meu-nome').textContent = eu.name;
        el('meu-avatar').textContent = iniciais(eu.name);

        this.servidores = await this.api.servers();
        this.desenharTrilha();

        el('compartilhar').onclick = () => this.compartilhar();
        el('parar').onclick = () => this.pararCompartilhamento();
        el('sair-voz').onclick = () => this.sairDaVoz();
        el('mudo').onclick = () => this.alternarMicrofone();
        el('config-mic').onclick = () => this.alternarPainelMic();
        this.ligarPainelMic();
    }

    /**
     * The panel is static markup wired once. The settings live in the gate, which is
     * what actually decides frame by frame whether the audio leaves this machine.
     */
    ligarPainelMic() {
        const { mode, threshold, pushKey, noiseSuppression } = this.mic.settings;

        el('mic-limiar').value = threshold;
        el('mic-ruido').checked = noiseSuppression;
        el('mic-atalho').textContent = pushKey;

        for (const opcao of document.querySelectorAll('input[name="modo-mic"]')) {
            opcao.checked = opcao.value === mode;
            opcao.onchange = () => {
                this.mic.save({ mode: opcao.value });
                el('mic-voz').hidden = opcao.value !== 'voice';
                el('mic-tecla').hidden = opcao.value !== 'ptt';
            };
        }

        el('mic-voz').hidden = mode !== 'voice';
        el('mic-tecla').hidden = mode !== 'ptt';

        el('mic-limiar').oninput = evento => this.mic.save({ threshold: Number(evento.target.value) });
        el('mic-ruido').onchange = evento => this.mic.save({ noiseSuppression: evento.target.checked });

        el('mic-atalho').onclick = () => {
            el('mic-atalho').textContent = 'press a key…';

            // `once` matters: without it every later keypress would keep rebinding.
            window.addEventListener('keydown', evento => {
                evento.preventDefault();
                this.mic.save({ pushKey: evento.code });
                el('mic-atalho').textContent = evento.code;
            }, { once: true, capture: true });
        };
    }

    alternarPainelMic() {
        el('painel-mic').hidden = ! el('painel-mic').hidden;
    }

    desenharTrilha() {
        const trilha = el('trilha');

        trilha.innerHTML = '';

        for (const servidor of this.servidores) {
            const botao = document.createElement('button');

            botao.className = `servidor${this.servidor?.id === servidor.id ? ' ativo' : ''}`;
            botao.textContent = servidor.initials;
            botao.title = servidor.name;
            botao.onclick = () => this.abrirServidor(servidor.id);
            trilha.appendChild(botao);
        }

        const separador = document.createElement('span');

        separador.className = 'separador';
        trilha.appendChild(separador);

        const novo = document.createElement('button');

        novo.className = 'servidor novo';
        novo.textContent = '+';
        novo.title = 'Criar servidor';
        novo.onclick = () => this.criarServidor();
        trilha.appendChild(novo);
    }

    async criarServidor() {
        const nome = prompt('Server name');

        if (! nome?.trim()) {
            return;
        }

        const servidor = await this.api.createServer(nome.trim());

        this.servidores = await this.api.servers();
        await this.abrirServidor(servidor.id);
    }

    async abrirServidor(id) {
        this.servidor = await this.api.server(id);

        el('nome-servidor').textContent = this.servidor.name;
        el('vazio').hidden = true;
        this.desenharTrilha();
        this.desenharCanais();

        const texto = this.servidor.channels.find(canal => canal.type === 'text');

        if (texto) {
            await this.abrirCanal(texto);
        }
    }

    desenharCanais() {
        const lista = el('lista-canais');

        lista.innerHTML = '';

        for (const tipo of ['text', 'voice']) {
            const canais = this.servidor.channels.filter(canal => canal.type === tipo);

            if (! canais.length) {
                continue;
            }

            const titulo = document.createElement('p');

            titulo.className = 'secao';
            titulo.textContent = tipo === 'text' ? 'Text channels' : 'Voice channels';
            lista.appendChild(titulo);

            for (const canal of canais) {
                const botao = document.createElement('button');

                botao.className = `canal${this.canal?.id === canal.id ? ' ativo' : ''}`;
                botao.innerHTML = tipo === 'text'
                    ? `<span style="font-size:20px;color:#80848e">#</span><span>${canal.name}</span>`
                    : `<span style="color:#80848e">🔊</span><span>${canal.name}</span><span class="relogio" data-relogio="${canal.id}"></span>`;
                botao.onclick = () => (tipo === 'text' ? this.abrirCanal(canal) : this.entrarNaVoz(canal));
                lista.appendChild(botao);

                if (tipo === 'voice') {
                    const membros = document.createElement('div');

                    membros.dataset.participantes = canal.id;
                    lista.appendChild(membros);
                }
            }
        }
    }

    async abrirCanal(canal) {
        this.canal = canal;
        el('titulo-canal').textContent = `# ${canal.name}`;
        this.desenharCanais();

        const mensagens = await this.api.messages(canal.id);

        el('palco').hidden = true;
        el('vazio').hidden = false;
        el('vazio').textContent = mensagens.length
            ? mensagens.map(mensagem => `${mensagem.author.name}: ${mensagem.content}`).join('\n')
            : 'No messages yet.';
    }

    async entrarNaVoz(canal) {
        el('meu-estado').textContent = 'connecting…';

        try {
            this.voz = await this.api.voiceToken(canal.id);
        } catch (falha) {
            el('meu-estado').textContent = falha.message;

            return;
        }

        try {
            // One client for everything: voice rides the SFU (which fans out to any
            // number of people), while the screen stays direct between machines.
            this.sfu = new SfuClient();
            this.sfu.addEventListener('newProducer', evento => this.consumir(evento.detail));

            this.p2p = new P2P(this.sfu, (de, stream) => this.mostrarTela(de, stream));

            const entrada = await this.sfu.connect(
                this.voz.url,
                async () => (await this.api.voiceToken(canal.id)).token,
            );

            await this.p2p.attach();

            this.participantes = entrada.peers.map(peer => peer.peerId);

            for (const peer of entrada.peers) {
                for (const producer of peer.producers) {
                    await this.consumir({ ...producer, peerId: peer.peerId, name: peer.name });
                }
            }

            await this.abrirMicrofone();
        } catch (falha) {
            el('meu-estado').textContent = `could not join the room: ${falha.message}`;
            this.p2p = null;
            this.sfu = null;

            return;
        }

        el('faixa-voz').hidden = false;
        el('voz-canal').textContent = canal.name;
        el('meu-estado').textContent = `in ${canal.name}`;
        el('palco').hidden = false;
        el('vazio').hidden = true;

        const inicio = Date.now();

        clearInterval(this.relogio);
        this.relogio = setInterval(() => {
            const segundos = Math.floor((Date.now() - inicio) / 1000);
            const marca = `${String(Math.floor(segundos / 60)).padStart(2, '0')}:${String(segundos % 60).padStart(2, '0')}`;

            el('voz-relogio').textContent = marca;

            const relogioDoCanal = document.querySelector(`[data-relogio="${canal.id}"]`);

            if (relogioDoCanal) {
                relogioDoCanal.textContent = marca;
            }
        }, 1000);
    }

    /** Draws (or removes) the screen of someone who is broadcasting. */
    mostrarTela(de, stream) {
        const existente = document.querySelector(`[data-tela="${de}"]`);

        if (! stream) {
            existente?.remove();

            return;
        }

        const quadro = existente ?? document.createElement('figure');

        quadro.className = 'tile';
        quadro.dataset.tela = de;
        quadro.innerHTML = '<video autoplay playsinline></video><figcaption></figcaption>';
        quadro.querySelector('video').srcObject = stream;
        quadro.querySelector('figcaption').textContent = 'broadcasting';

        if (! existente) {
            el('palco').appendChild(quadro);
        }

        const total = el('palco').childElementCount;

        el('palco').style.gridTemplateColumns = `repeat(${total > 1 ? 2 : 1}, minmax(0, 1fr))`;
    }

    /**
     * Voice goes through the SFU, not P2P: audio is cheap and the server already fans it
     * out to everyone in the room, so talking works with any number of people — while the
     * screen, which is expensive, stays direct between machines.
     */
    async abrirMicrofone() {
        try {
            const trilha = await this.mic.open();

            await this.sfu.publishMicrophone(trilha);
            this.pintarMicrofone({ db: MicrophoneGate.FLOOR_DB, transmitting: false, muted: false });
        } catch (falha) {
            this.micNegado = true;
            el('meu-estado').textContent = `microphone unavailable: ${falha.message}`;
        }
    }

    /** Someone else's audio or screen arriving through the SFU. */
    async consumir({ producerId }) {
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
            this.mostrarTela(peerId, new MediaStream([consumer.track]));
        } catch (falha) {
            console.warn('could not receive media:', falha);
        }
    }

    /** Mute is the gate, never the producer: the call keeps the audio path warm. */
    alternarMicrofone() {
        if (! this.mic.active) {
            el('meu-estado').textContent = this.micNegado ? 'the system denied the microphone' : 'join a voice channel first';

            return;
        }

        this.mic.setMuted(! this.mic.muted);
    }

    pintarMicrofone({ transmitting, muted, db }) {
        const botao = el('mudo');

        if (botao) {
            botao.textContent = muted ? 'Unmute' : 'Mute';
            botao.classList.toggle('perigo', muted);
        }

        const medidor = document.querySelector('[data-medidor]');

        if (medidor) {
            medidor.style.width = `${MicrophoneGate.toFraction(db) * 100}%`;
            medidor.style.background = transmitting ? '#23a55a' : '#4e5058';
        }
    }

    async sairDaVoz() {
        clearInterval(this.relogio);
        await this.pararCompartilhamento();
        this.mic.close();
        this.micNegado = false;
        await this.sfu?.leaveRoom();
        this.sfu?.disconnect();
        this.p2p?.close();
        this.p2p = null;
        this.sfu = null;
        this.voz = null;
        document.querySelectorAll('audio[data-remoto]').forEach(elemento => elemento.remove());
        el('faixa-voz').hidden = true;
        el('palco').hidden = true;
        el('vazio').hidden = false;
        el('meu-estado').textContent = 'Available';
    }

    async compartilhar() {
        if (! this.p2p) {
            el('meu-estado').textContent = 'join a voice channel first';

            return;
        }

        try {
            await this.p2p.broadcast(el('qualidade').value, this.participantes);
            el('compartilhar').hidden = true;
            el('parar').hidden = false;
            el('meu-estado').textContent = this.participantes.length
                ? `broadcasting to ${this.participantes.length}`
                : 'broadcasting (no one watching yet)';
        } catch (falha) {
            el('meu-estado').textContent = falha.message ?? String(falha);
        }
    }

    async pararCompartilhamento() {
        const quadros = await this.p2p?.stop().catch(() => 0);

        el('compartilhar').hidden = false;
        el('parar').hidden = true;

        if (quadros) {
            el('meu-estado').textContent = `${quadros} frames broadcast`;
        }
    }
}

new App().start();
