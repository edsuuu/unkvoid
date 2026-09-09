import { Device } from 'mediasoup-client';

export class SfuClient extends EventTarget {
    /**
     * Quanto esperar por uma resposta do servidor de mídia.
     *
     * Generoso de propósito: 10s cobre uma rede ruim sem transformar lentidão em erro,
     * e ainda assim não deixa nada pendurado.
     */
    static REQUEST_TIMEOUT_MS = 10_000;

    /**
     * Qual implementação de WebRTC o mediasoup-client deve usar.
     *
     * Ele descobre isso farejando o user-agent, e o WKWebView do app **não põe o token
     * `Safari`** no dele. O teste do mediasoup exige essa palavra, a detecção devolve
     * `undefined`, e o `load()` estoura com "device not supported" — a voz nunca
     * funcionou dentro do app por causa de uma palavra que falta numa string.
     *
     * Onde a detecção funciona (Chrome, Edge, Firefox, Safari de verdade), este método
     * não opina: devolve vazio e deixa o mediasoup escolher.
     */
    static handler() {
        const agent = navigator.userAgent;
        const webkitSemChrome = /AppleWebKit/i.test(agent) && ! /Chrome|Chromium|Edg/i.test(agent);

        return webkitSemChrome && ! /\bSafari\b/i.test(agent) ? { handlerName: 'Safari12' } : {};
    }

    constructor() {
        super();
        this.socket = null;
        this.device = null;
        this.recvTransport = null;
        this.pending = new Map();
        this.nextRequestId = 1;
        this.consumers = new Map();
        this.peerId = null;
        this.peers = new Map();
        this.identity = null;
        this.resumeKey = null;
        this.url = null;
        this.closedByUs = false;
        this.reconnectAttempt = 0;
        this.reconnectTimer = null;
        this.lastRttMs = null;
    }

    emit(name, detail) {
        this.dispatchEvent(new CustomEvent(name, { detail }));
    }

    /** `identity` e `{ room, name }`: o codigo da sala e como voce aparece para os outros. */
    connect(url, identity) {
        this.url = url;
        this.identity = identity;
        this.closedByUs = false;

        return this.openSocket().then(() => this.setup());
    }

    openSocket() {
        return new Promise((resolve, reject) => {
            this.socket = new WebSocket(this.url);
            this.socket.onerror = () => reject(new Error('unable to open the WebSocket'));
            this.socket.onmessage = message => this.handleMessage(JSON.parse(message.data));
            this.socket.onclose = () => this.handleClose();
            this.socket.onopen = () => resolve();
        });
    }

    /**
     * WebRTC media does not drop with signaling. So a socket drop is
     * treated as a hiccup: it retries with backoff and, if the server still has the
     * session, no one notices. If not, it republishes everything from scratch.
     */
    handleClose() {
        this.emit('diagnostic', { event: 'socket.close', data: { attempt: this.reconnectAttempt } });
        for (const waiting of this.pending.values()) {
            waiting.reject(new Error('connection dropped'));
        }

        this.pending.clear();

        if (this.closedByUs) {
            this.emit('closed');

            return;
        }

        this.emit('reconnecting', { attempt: this.reconnectAttempt + 1 });
        this.scheduleReconnect();
    }

    scheduleReconnect() {
        if (this.reconnectAttempt >= 8) {
            this.emit('closed');

            return;
        }

        const delay = Math.min(1000 * 2 ** this.reconnectAttempt, 10000);

        this.reconnectAttempt += 1;
        this.reconnectTimer = setTimeout(() => void this.reconnect(), delay);
    }

    async reconnect() {
        try {
            await this.openSocket();

            const joined = await this.setup();

            this.reconnectAttempt = 0;
            this.emit('reconnected', { resumed: joined.resumed });
        } catch (error) {
            this.emit('reconnecting', { attempt: this.reconnectAttempt, error: error.message });
            this.scheduleReconnect();
        }
    }

    handleMessage(message) {
        if (message.event) {
            this.emit('diagnostic', { event: `sfu.${message.event}`, data: message.data });
        }
        if (message.event) {
            this.trackPeers(message.event, message.data);
            this.emit(message.event, message.data);

            return;
        }

        const waiting = this.pending.get(message.id);

        if (!waiting) {
            return;
        }

        this.pending.delete(message.id);
        message.ok ? waiting.resolve(message.data) : waiting.reject(new Error(message.error));
    }

    trackPeers(event, data) {
        if (event === 'peerJoined') {
            this.peers.set(data.peerId, { name: data.name, sharing: false });
        }

        if (event === 'peerLeft') {
            this.peers.delete(data.peerId);
        }

        if (event === 'peerConnectionLost' || event === 'peerReconnected') {
            const peer = this.peers.get(data.peerId);

            if (peer) {
                peer.reconnecting = event === 'peerConnectionLost';
            }
        }

        if (event === 'newProducer' && data.source === 'screen') {
            this.markSharing(data.peerId, true);
        }

        if (event === 'producerClosed' && data.source === 'screen') {
            this.markSharing(data.peerId, false);
        }

        if (event === 'consumerClosed') {
            this.consumers.delete(data.consumerId);
        }

        this.emit('peersChanged', [...this.peers.entries()]);
    }

    markSharing(peerId, sharing) {
        const peer = this.peers.get(peerId);

        if (peer) {
            peer.sharing = sharing;
        }
    }

    /**
     * Uma requisição ao servidor de mídia.
     *
     * O prazo não é zelo: sem ele, um pedido que o servidor não responde fica pendurado
     * para sempre com o socket aberto. Quando isso acontece com o `leave`, o
     * `leaveVoice` trava antes do `disconnect()`, o socket segue vivo, e para todo mundo
     * — inclusive para a web — você continua na sala. Não são os 45s de carência: é
     * para sempre. Cair fora é melhor do que virar fantasma.
     */
    request(action, data = {}) {
        const id = this.nextRequestId++;
        const startedAt = performance.now();

        return new Promise((resolve, reject) => {
            const prazo = setTimeout(() => {
                this.pending.delete(id);
                reject(new Error(`o servidor não respondeu a "${action}"`));
            }, SfuClient.REQUEST_TIMEOUT_MS);

            const encerrar = fim => valor => {
                clearTimeout(prazo);
                this.lastRttMs = Math.round(performance.now() - startedAt);
                fim(valor);
            };

            this.pending.set(id, { resolve: encerrar(resolve), reject: encerrar(reject) });

            try {
                this.socket.send(JSON.stringify({ id, action, data }));
            } catch (falha) {
                clearTimeout(prazo);
                this.pending.delete(id);
                reject(falha);
            }
        });
    }

    async setup() {
        // A `resumeKey` e o que prova ser a mesma pessoa depois de uma queda. Ela so vale
        // com `resume`, e so pedimos resume se o transporte desta sessao continua vivo:
        // reaberto o app, ele e novo em folha e retomar deixaria o cliente sem transporte.
        const joined = await this.request('join', {
            ...this.identity,
            resumeKey: this.resumeKey,
            resume: Boolean(this.recvTransport),
        });

        this.peerId = joined.peerId;
        this.resumeKey = joined.resumeKey;

        // Retomada: transportes, producers e consumers do servidor seguem ativos.
        if (joined.resumed) {
            return joined;
        }

        this.consumers.clear();
        this.peers.set(joined.peerId, { name: joined.name, self: true, sharing: false });

        for (const peer of joined.peers) {
            this.peers.set(peer.peerId, {
                name: peer.name,
                sharing: peer.producers.some(producer => producer.source === 'screen'),
            });
        }

        this.device = new Device(SfuClient.handler());
        await this.device.load({ routerRtpCapabilities: joined.routerRtpCapabilities });

        this.recvTransport = await this.createTransport();

        return joined;
    }

    /**
     * So o de recepcao. Publicar nao passa por WebRTC: a tela sobe como RTP puro, direto
     * do Rust para a porta que o servidor devolve no `producePlain`.
     */
    async createTransport() {
        const params = await this.request('createTransport');
        const options = {
            id: params.transportId,
            iceParameters: params.iceParameters,
            iceCandidates: params.iceCandidates,
            dtlsParameters: params.dtlsParameters,
        };

        const transport = this.device.createRecvTransport(options);

        transport.on('connect', ({ dtlsParameters }, callback, errback) =>
            this.request('connectTransport', { transportId: transport.id, dtlsParameters })
                .then(callback)
                .catch(errback));

        return transport;
    }

    async consume(producerId) {
        const params = await this.request('consume', {
            transportId: this.recvTransport.id,
            producerId,
            rtpCapabilities: this.device.rtpCapabilities,
        });

        const consumer = await this.recvTransport.consume({
            id: params.consumerId,
            producerId: params.producerId,
            kind: params.kind,
            rtpParameters: params.rtpParameters,
        });

        this.consumers.set(consumer.id, consumer);
        await this.request('resumeConsumer', { consumerId: consumer.id });

        return { consumer, ...params };
    }

    /** Explicit departure: without this, the server treats it as a drop and the person becomes a ghost. */
    async leaveRoom() {
        await this.request('leave').catch(() => {});
    }

    disconnect() {
        this.closedByUs = true;
        clearTimeout(this.reconnectTimer);
        this.socket?.close();
        this.recvTransport?.close();
        this.consumers.clear();
    }
}
