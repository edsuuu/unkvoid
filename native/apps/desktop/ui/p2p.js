import { Signaling } from './signaling.js';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const STUN = ['stun:stun.l.google.com:19302'];

/**
 * Une os dois lados da transmissão.
 *
 * Quem envia usa o Rust: captura nativa e encoder por hardware, sem barra do
 * navegador. Quem recebe usa o WebRTC do próprio webview — receber vídeo ele faz
 * bem, e assim não é preciso decodificar nem desenhar em Rust.
 *
 * Acima de 3 espectadores o upload de quem transmite multiplica (4 pessoas em 1080p
 * ≈ 28 Mbps de subida) e o SFU volta a compensar.
 */
export class P2P {
    static LIMITE_P2P = 3;

    constructor(aoReceberTela) {
        this.sinal = new Signaling();
        this.recebendo = new Map();
        this.transmitindo = false;
        this.aoReceberTela = aoReceberTela;
    }

    async join(url, token) {
        this.sinal.addEventListener('signal', evento => this.tratarSinal(evento.detail));
        this.sinal.addEventListener('peerLeft', evento => this.encerrarRecepcao(evento.detail.peerId));

        const entrada = await this.sinal.connect(url, token);

        this.sinal.addEventListener('peerJoined', evento => this.oferecerA(evento.detail.peerId));

        // Cada conexão do lado Rust manda os próprios candidatos, já endereçados.
        await listen('p2p:signal', evento => {
            const [destino, candidato] = evento.payload;

            void this.sinal.signal(destino, 'candidate', { candidate: candidato });
        });

        return entrada;
    }

    /**
     * Começa a transmitir para os participantes informados. Uma oferta por
     * espectador: no P2P cada um precisa da própria conexão.
     */
    async broadcast(quality, espectadores) {
        if (espectadores.length > P2P.LIMITE_P2P) {
            throw new Error(`P2P vai até ${P2P.LIMITE_P2P} espectadores — acima disso o upload multiplica`);
        }

        await invoke('start_broadcast', { quality, iceServers: STUN });

        this.transmitindo = true;

        for (const espectador of espectadores) {
            await this.oferecerA(espectador);
        }
    }

    /** Uma conexão por espectador — inclusive quem chega depois de começar. */
    async oferecerA(peerId) {
        if (! this.transmitindo || peerId === this.sinal.peerId) {
            return;
        }

        try {
            const sdp = await invoke('offer_to', { peerId });

            await this.sinal.signal(peerId, 'offer', { sdp });
        } catch (falha) {
            console.warn(`não ofereceu para ${peerId}:`, falha);
        }
    }

    async stop() {
        if (! this.transmitindo) {
            return 0;
        }

        this.transmitindo = false;

        return invoke('stop_broadcast');
    }

    async tratarSinal({ from, kind, payload }) {
        if (kind === 'offer') {
            await this.receberOferta(from, payload.sdp);

            return;
        }

        if (kind === 'answer') {
            await invoke('accept_answer', { peerId: from, sdp: payload.sdp });

            return;
        }

        if (kind === 'candidate') {
            await this.adicionarCandidato(from, payload.candidate);
        }
    }

    /** Lado de quem assiste: o webview monta a conexão e entrega o vídeo pronto. */
    async receberOferta(de, sdp) {
        this.encerrarRecepcao(de);

        const conexao = new RTCPeerConnection({ iceServers: [{ urls: STUN }] });

        this.recebendo.set(de, conexao);

        conexao.onicecandidate = evento => {
            if (evento.candidate) {
                void this.sinal.signal(de, 'candidate', { candidate: JSON.stringify(evento.candidate.toJSON()) });
            }
        };

        conexao.ontrack = evento => this.aoReceberTela(de, evento.streams[0] ?? new MediaStream([evento.track]));

        await conexao.setRemoteDescription({ type: 'offer', sdp });

        const resposta = await conexao.createAnswer();

        await conexao.setLocalDescription(resposta);
        await this.sinal.signal(de, 'answer', { sdp: resposta.sdp });
    }

    async adicionarCandidato(de, json) {
        const conexao = this.recebendo.get(de);

        // Sem conexão de recepção, o candidato é para o lado que transmite.
        if (! conexao) {
            await invoke('add_candidate', { peerId: de, candidate: json }).catch(() => {});

            return;
        }

        await conexao.addIceCandidate(JSON.parse(json)).catch(() => {});
    }

    encerrarRecepcao(de) {
        const conexao = this.recebendo.get(de);

        if (conexao) {
            conexao.close();
            this.recebendo.delete(de);
            this.aoReceberTela(de, null);
        }

        // Quem saiu também deixa de ser espectador da minha transmissão.
        void invoke('drop_viewer', { peerId: de }).catch(() => {});
    }

    close() {
        for (const de of [...this.recebendo.keys()]) {
            this.encerrarRecepcao(de);
        }

        this.sinal.close();
    }
}
