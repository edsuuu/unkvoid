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
        this.p2p = null;
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
            this.p2p = new P2P((de, stream) => this.mostrarTela(de, stream));

            const entrada = await this.p2p.join(this.voz.url, this.voz.token);

            this.participantes = entrada.peers.map(peer => peer.peerId);
        } catch (falha) {
            el('meu-estado').textContent = `could not join the room: ${falha.message}`;
            this.p2p = null;

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

    async sairDaVoz() {
        clearInterval(this.relogio);
        await this.pararCompartilhamento();
        this.p2p?.close();
        this.p2p = null;
        this.voz = null;
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
