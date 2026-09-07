const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const STUN = ['stun:stun.l.google.com:19302'];

/**
 * Connects the two sides of the broadcast.
 *
 * The sender uses Rust: native capture and hardware encoding, without a browser
 * bar. The receiver uses the webview's WebRTC — it handles video well, so there
 * is no need to decode or render in Rust.
 *
 * It rides on the SfuClient's socket instead of opening its own. Two sockets would
 * be two sessions with the same participant id, and the SFU replaces the older one:
 * joining would knock out the voice that had just connected.
 *
 * Above 3 viewers the broadcaster's upload multiplies (4 people at 1080p ≈ 28 Mbps
 * upstream), and the SFU becomes worthwhile again.
 */
export class P2P {
    static LIMITE_P2P = 3;

    constructor(sfu, aoReceberTela) {
        this.sfu = sfu;
        this.recebendo = new Map();
        this.transmitindo = false;
        this.aoReceberTela = aoReceberTela;
    }

    async attach() {
        this.sfu.addEventListener('signal', evento => this.tratarSinal(evento.detail));
        this.sfu.addEventListener('peerLeft', evento => this.encerrarRecepcao(evento.detail.peerId));
        this.sfu.addEventListener('peerJoined', evento => this.oferecerA(evento.detail.peerId));

        // Each Rust-side connection sends its own, already-addressed candidates.
        await listen('p2p:signal', evento => {
            const [destino, candidato] = evento.payload;

            void this.sfu.signal(destino, 'candidate', { candidate: candidato });
        });
    }

    /**
     * Starts broadcasting to the specified participants. One offer per viewer:
     * in P2P, each needs its own connection.
     */
    async broadcast(quality, espectadores) {
        if (espectadores.length > P2P.LIMITE_P2P) {
            throw new Error(`P2P supports up to ${P2P.LIMITE_P2P} viewers — upload multiplies above that`);
        }

        await invoke('start_broadcast', { quality, iceServers: STUN });

        this.transmitindo = true;

        for (const espectador of espectadores) {
            await this.oferecerA(espectador);
        }
    }

    /** One connection per viewer — including those who join after it starts. */
    async oferecerA(peerId) {
        if (! this.transmitindo || peerId === this.sfu.peerId) {
            return;
        }

        try {
            const sdp = await invoke('offer_to', { peerId });

            await this.sfu.signal(peerId, 'offer', { sdp });
        } catch (falha) {
            console.warn(`could not offer to ${peerId}:`, falha);
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

    /** Viewer side: the webview builds the connection and delivers ready-to-play video. */
    async receberOferta(de, sdp) {
        this.encerrarRecepcao(de);

        const conexao = new RTCPeerConnection({ iceServers: [{ urls: STUN }] });

        this.recebendo.set(de, conexao);

        conexao.onicecandidate = evento => {
            if (evento.candidate) {
                void this.sfu.signal(de, 'candidate', { candidate: JSON.stringify(evento.candidate.toJSON()) });
            }
        };

        conexao.ontrack = evento => this.aoReceberTela(de, evento.streams[0] ?? new MediaStream([evento.track]));

        await conexao.setRemoteDescription({ type: 'offer', sdp });

        const resposta = await conexao.createAnswer();

        await conexao.setLocalDescription(resposta);
        await this.sfu.signal(de, 'answer', { sdp: resposta.sdp });
    }

    async adicionarCandidato(de, json) {
        const conexao = this.recebendo.get(de);

        // Without a receiving connection, the candidate is for the broadcasting side.
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

        // Someone who left is no longer a viewer of my broadcast.
        void invoke('drop_viewer', { peerId: de }).catch(() => {});
    }

    close() {
        for (const de of [...this.recebendo.keys()]) {
            this.encerrarRecepcao(de);
        }
    }
}
