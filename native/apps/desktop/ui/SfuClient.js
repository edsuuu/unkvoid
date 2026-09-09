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
        const webkitWithoutChrome = /AppleWebKit/i.test(agent) && ! /Chrome|Chromium|Edg/i.test(agent);

        return webkitWithoutChrome && ! /\bSafari\b/i.test(agent) ? { handlerName: 'Safari12' } : {};
    }

    constructor() {
        super();
        this.socket = null;
        this.device = null;
        this.recvTransport = null;
        this.pending = new Map();
        this.nextRequestId = 1;
        this.consumers = new Map();
        this.consumerPeers = new Map();
        this.peerLatency = new Map();
        this.peerId = null;
        this.peers = new Map();
        this.identity = null;
        this.resumeKey = null;
        this.url = null;

        /** Os codecs de vídeo que esta máquina aceita. Sem H.264 aqui, não há imagem. */
        this.videoCodecs = [];
        this.closedByUs = false;
        this.reconnectAttempt = 0;
        this.reconnectTimer = null;
        this.socketGeneration = 0;
        this.lastRttMs = null;

        /** Ida e volta do transporte de mídia. É este o ping que a barra mostra. */
        this.transportRttMs = null;
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
            const generation = ++this.socketGeneration;
            const socket = new WebSocket(this.url);

            this.socket = socket;
            socket.onerror = () => {
                if (generation === this.socketGeneration) {
                    reject(new Error('unable to open the WebSocket'));
                }
            };
            socket.onmessage = message => {
                if (generation === this.socketGeneration) {
                    this.handleMessage(JSON.parse(message.data));
                }
            };
            socket.onclose = () => {
                if (generation === this.socketGeneration) {
                    this.handleClose();
                }
            };
            socket.onopen = () => {
                if (generation === this.socketGeneration) {
                    resolve();
                }
            };
        });
    }

    /**
     * A mídia do WebRTC não cai junto com a sinalização. Por isso um socket que cai é
     * tratado como engasgo: tenta de novo com espera crescente e, se o servidor ainda
     * tiver a sessão, ninguém percebe. Se não tiver, republica tudo do zero.
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
        if (this.reconnectTimer) {
            return;
        }

        if (this.reconnectAttempt >= 8) {
            this.emit('closed');

            return;
        }

        const delay = Math.min(1000 * 2 ** this.reconnectAttempt, 10000);

        this.reconnectAttempt += 1;
        this.reconnectTimer = setTimeout(() => {
            this.reconnectTimer = null;
            void this.reconnect();
        }, delay);
    }

    async reconnect() {
        try {
            await this.openSocket();

            const joined = await this.setup();

            this.reconnectAttempt = 0;
            this.emit('reconnected', { resumed: joined.resumed, peers: joined.peers ?? [] });
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
            this.peers.set(data.peerId, { peerId: data.peerId, name: data.name, sharing: false, producers: [] });
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

        if (event === 'newProducer') {
            this.trackProducer(data.peerId, data);
        }

        if (event === 'producerClosed') {
            this.forgetProducer(data.peerId, data.producerId);
        }

        if (event === 'consumerClosed') {
            this.consumers.delete(data.consumerId);
            this.consumerPeers.delete(data.consumerId);
        }

        this.emit('peersChanged', [...this.peers.entries()]);
    }

    /**
     * A lista de producers de cada pessoa, mantida viva.
     *
     * `newProducer` chega uma vez e nunca mais. Quem perdeu esse instante — porque o
     * consumo falhou, porque a pessoa pausou — nao tinha como voltar a pedir a tela sem
     * sair e entrar na sala de novo. Guardar o id e o que deixa o botao "assistir" existir.
     */
    trackProducer(peerId, { producerId, kind, source }) {
        const peer = this.peers.get(peerId);

        if (! peer) {
            return;
        }

        peer.producers = [...(peer.producers ?? []).filter(item => item.producerId !== producerId), { producerId, kind, source }];
        peer.sharing = peer.producers.some(item => item.source === 'screen');
    }

    forgetProducer(peerId, producerId) {
        const peer = this.peers.get(peerId);

        if (! peer) {
            return;
        }

        peer.producers = (peer.producers ?? []).filter(item => item.producerId !== producerId);
        peer.sharing = peer.producers.some(item => item.source === 'screen');
    }

    /** Os consumers que carregam a midia de uma pessoa — o que pausar quando ninguem quer ver. */
    consumersOf(peerId) {
        return [...this.consumerPeers].filter(([, owner]) => owner === peerId).map(([consumerId]) => consumerId);
    }

    /**
     * Pausa no servidor, nao so no elemento `<video>`.
     *
     * Parar o video sozinho continuaria baixando e decodificando tudo: o custo que
     * incomoda quem so quer ouvir esta no decoder, e ele so para quando o pacote deixa
     * de chegar. `pauseConsumer` e o unico jeito de o pacote deixar de chegar.
     */
    async setPeerPaused(peerId, paused) {
        const action = paused ? 'pauseConsumer' : 'resumeConsumer';

        await Promise.all(this.consumersOf(peerId).map(consumerId => this.request(action, { consumerId })));
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
            const deadline = setTimeout(() => {
                this.pending.delete(id);
                reject(new Error(`o servidor não respondeu a "${action}"`));
            }, SfuClient.REQUEST_TIMEOUT_MS);

            const settle = finish => value => {
                clearTimeout(deadline);
                this.lastRttMs = Math.round(performance.now() - startedAt);
                finish(value);
            };

            this.pending.set(id, { resolve: settle(resolve), reject: settle(reject) });

            try {
                this.socket.send(JSON.stringify({ id, action, data }));
            } catch (failure) {
                clearTimeout(deadline);
                this.pending.delete(id);
                reject(failure);
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
        // Uma abertura que falhou pode reconectar com outro id. Remonta a lista de
        // participantes pelo retrato do servidor em vez de guardar sessão velha.
        this.peers.clear();
        this.consumerPeers.clear();
        this.peerLatency.clear();
        this.peers.set(joined.peerId, { peerId: joined.peerId, name: joined.name, self: true, sharing: false, producers: [] });

        for (const peer of joined.peers) {
            this.peers.set(peer.peerId, {
                peerId: peer.peerId,
                name: peer.name,
                producers: peer.producers,
                sharing: peer.producers.some(producer => producer.source === 'screen'),
            });
        }

        this.device = new Device(SfuClient.handler());
        await this.device.load({ routerRtpCapabilities: joined.routerRtpCapabilities });

        // Se o H.264 não estiver aqui, o servidor recusa o `consume` e a tela fica preta
        // sem erro nenhum. É o modo de falha mais caro do projeto no Linux, e a única
        // forma de vê-lo é esta linha no diagnóstico.
        this.videoCodecs = this.device.rtpCapabilities.codecs
            .filter(codec => codec.kind === 'video')
            .map(codec => codec.mimeType);

        this.emit('diagnostic', {
            event: 'device.ready',
            data: { handler: this.device.handlerName, video: this.videoCodecs },
        });

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
        this.consumerPeers.set(consumer.id, params.peerId);
        await this.request('resumeConsumer', { consumerId: consumer.id });

        return { consumer, ...params };
    }

    consumersHasProducer(producerId) {
        return [...this.consumers.values()].some(consumer => consumer.producerId === producerId);
    }

    async updatePeerLatency() {
        if (! this.recvTransport) {
            return;
        }

        const stats = await this.recvTransport.getStats();
        const transport = [...stats.values()].find(report =>
            report.type === 'candidate-pair' && report.state === 'succeeded' && report.nominated);
        const rtt = transport?.currentRoundTripTime;

        this.transportRttMs = rtt != null ? Math.round(rtt * 1000) : null;

        for (const peerId of this.peers.keys()) {
            this.peerLatency.set(peerId, this.transportRttMs);
        }

        for (const [consumerId, peerId] of this.consumerPeers) {
            const inbound = [...stats.values()].find(report =>
                report.type === 'inbound-rtp' && report.ssrc === this.consumers.get(consumerId)?.rtpParameters?.encodings?.[0]?.ssrc);
            const jitter = inbound?.jitter;
            this.peerLatency.set(peerId, rtt != null ? Math.round(rtt * 1000) : jitter != null ? Math.round(jitter * 2000) : null);
        }
    }

    /** Sair de propósito: sem isto o servidor trata como queda e a pessoa vira fantasma. */
    async leaveRoom() {
        await this.request('leave').catch(() => {});
    }

    disconnect() {
        this.closedByUs = true;
        clearTimeout(this.reconnectTimer);
        this.reconnectTimer = null;
        this.socketGeneration += 1;
        this.socket?.close();
        this.recvTransport?.close();
        this.consumers.clear();
        this.consumerPeers.clear();
        this.peerLatency.clear();
    }
}
