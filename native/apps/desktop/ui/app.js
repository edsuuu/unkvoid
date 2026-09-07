import { Api } from './api.js';
import { P2P } from './p2p.js';

const { invoke } = window.__TAURI__.core;
const { openUrl } = window.__TAURI__.opener;
const { onOpenUrl } = window.__TAURI__.deepLink;

const el = id => document.getElementById(id);
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
     * Ordem de abertura, igual ao Discord: atualiza primeiro, exige servidor depois,
     * e só então pede login. Entrar num app desatualizado ou sem servidor só geraria
     * erro mais adiante.
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
                el('update-status').textContent = `Instalando a versão ${versao}…`;
                await invoke('restart');
            }
        } catch (falha) {
            // Falha de update não pode impedir de abrir: o app segue na versão atual.
            console.warn('update indisponível:', falha);
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
        el('offline-tentativa').textContent = `tentativa ${this.tentativa}`;

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

        // O Google não abre dentro do app: vai para o navegador do sistema e volta
        // por deep link. Assim a senha nunca passa pela janela do Discord 2.0.
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
        const nome = prompt('Nome do servidor');

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
            titulo.textContent = tipo === 'text' ? 'Canais de texto' : 'Canais de voz';
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
            : 'Nenhuma mensagem ainda.';
    }

    async entrarNaVoz(canal) {
        el('meu-estado').textContent = 'conectando…';

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
            el('meu-estado').textContent = `não entrou na sala: ${falha.message}`;
            this.p2p = null;

            return;
        }

        el('faixa-voz').hidden = false;
        el('voz-canal').textContent = canal.name;
        el('meu-estado').textContent = `em ${canal.name}`;
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

    /** Desenha (ou remove) a tela de quem está transmitindo. */
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
        quadro.querySelector('figcaption').textContent = 'transmitindo';

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
        el('meu-estado').textContent = 'Disponível';
    }

    async compartilhar() {
        if (! this.p2p) {
            el('meu-estado').textContent = 'entre num canal de voz primeiro';

            return;
        }

        try {
            await this.p2p.broadcast(el('qualidade').value, this.participantes);
            el('compartilhar').hidden = true;
            el('parar').hidden = false;
            el('meu-estado').textContent = this.participantes.length
                ? `transmitindo para ${this.participantes.length}`
                : 'transmitindo (ninguém assistindo ainda)';
        } catch (falha) {
            el('meu-estado').textContent = falha.message ?? String(falha);
        }
    }

    async pararCompartilhamento() {
        const quadros = await this.p2p?.stop().catch(() => 0);

        el('compartilhar').hidden = false;
        el('parar').hidden = true;

        if (quadros) {
            el('meu-estado').textContent = `${quadros} quadros transmitidos`;
        }
    }
}

new App().start();
