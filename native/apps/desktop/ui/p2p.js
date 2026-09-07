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

    constructor(sfu, onScreen) {
        this.sfu = sfu;
        this.receiving = new Map();
        this.viewers = new Set();
        this.broadcasting = false;
        this.onSfu = false;
        this.onScreen = onScreen;
    }

    async attach() {
        this.sfu.addEventListener('signal', event => this.handleSignal(event.detail));
        this.sfu.addEventListener('peerLeft', event => this.endReception(event.detail.peerId));
        this.sfu.addEventListener('peerJoined', event => this.offerTo(event.detail.peerId));

        // Each Rust-side connection sends its own, already-addressed candidates.
        await listen('p2p:signal', event => {
            const [target, candidate] = event.payload;

            void this.sfu.signal(target, 'candidate', { candidate: candidate });
        });
    }

    /**
     * Starts broadcasting. Up to three viewers it goes direct, which is the fast path:
     * the server is in the US and the people are in Brazil, so going through it costs
     * about 139 ms instead of 20. Past that, it moves to the server, where the upload
     * stops depending on how many people are watching.
     */
    async broadcast(quality, viewers) {
        await invoke('start_broadcast', { quality, iceServers: STUN });

        this.broadcasting = true;

        if (viewers.length > P2P.LIMITE_P2P) {
            await this.moveToSfu();

            return;
        }

        for (const espectador of viewers) {
            await this.offerTo(espectador);
        }
    }

    /** One connection per viewer — including those who join after it starts. */
    async offerTo(peerId) {
        if (! this.broadcasting || peerId === this.sfu.peerId) {
            return;
        }

        // The person who has just arrived is the one who tips the balance: from here on
        // the direct path costs more upload than the server does.
        if (! this.onSfu && this.directViewers().length >= P2P.LIMITE_P2P) {
            await this.moveToSfu();

            return;
        }

        if (this.onSfu) {
            return;
        }

        try {
            const sdp = await invoke('offer_to', { peerId });

            this.viewers.add(peerId);
            await this.sfu.signal(peerId, 'offer', { sdp });
        } catch (failure) {
            console.warn(`could not offer to ${peerId}:`, failure);
        }
    }

    /** Direct connections currently carrying this broadcast. */
    directViewers() {
        return [...this.viewers];
    }

    /**
     * Hands the broadcast over to the server. Rust says what it will send — codec, SSRC
     * and the SRTP key — the server answers with where to send it, and from then on the
     * viewers consume it like any other producer, including the ones on the web.
     */
    async moveToSfu() {
        if (this.onSfu) {
            return;
        }

        this.onSfu = true;

        for (const kind of ['video', 'audio']) {
            const offer = await invoke('sfu_offer', { kind });
            const target = await this.sfu.request('producePlain', {
                kind,
                source: kind === 'video' ? 'screen' : 'screenAudio',
                ...offer,
            });

            if (kind === 'video') {
                await invoke('use_sfu', { address: `${target.ip}:${target.port}` });
            }
        }

        this.viewers.clear();
    }

    async stop() {
        if (! this.broadcasting) {
            return 0;
        }

        this.broadcasting = false;
        this.onSfu = false;
        this.viewers.clear();

        return invoke('stop_broadcast');
    }

    async handleSignal({ from, kind, payload }) {
        if (kind === 'offer') {
            await this.receiveOffer(from, payload.sdp);

            return;
        }

        if (kind === 'answer') {
            await invoke('accept_answer', { peerId: from, sdp: payload.sdp });

            return;
        }

        if (kind === 'candidate') {
            await this.addCandidate(from, payload.candidate);
        }
    }

    /** Viewer side: the webview builds the connection and delivers ready-to-play video. */
    async receiveOffer(from, sdp) {
        this.endReception(from);

        const connection = new RTCPeerConnection({ iceServers: [{ urls: STUN }] });

        this.receiving.set(from, connection);

        connection.onicecandidate = event => {
            if (event.candidate) {
                void this.sfu.signal(from, 'candidate', { candidate: JSON.stringify(event.candidate.toJSON()) });
            }
        };

        connection.ontrack = event => this.onScreen(from, event.streams[0] ?? new MediaStream([event.track]));

        await connection.setRemoteDescription({ type: 'offer', sdp });

        const answer = await connection.createAnswer();

        await connection.setLocalDescription(answer);
        await this.sfu.signal(from, 'answer', { sdp: answer.sdp });
    }

    async addCandidate(from, json) {
        const connection = this.receiving.get(from);

        // Without a receiving connection, the candidate is for the broadcasting side.
        if (! connection) {
            await invoke('add_candidate', { peerId: from, candidate: json }).catch(() => {});

            return;
        }

        await connection.addIceCandidate(JSON.parse(json)).catch(() => {});
    }

    endReception(from) {
        const connection = this.receiving.get(from);

        if (connection) {
            connection.close();
            this.receiving.delete(from);
            this.onScreen(from, null);
        }

        // Someone who left is no longer a viewer of my broadcast.
        this.viewers.delete(from);
        void invoke('drop_viewer', { peerId: from }).catch(() => {});
    }

    close() {
        for (const from of [...this.receiving.keys()]) {
            this.endReception(from);
        }
    }
}
