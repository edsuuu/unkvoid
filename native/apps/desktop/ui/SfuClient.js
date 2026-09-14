import { Device } from 'mediasoup-client';

/** Folga de reprodução da tela compartilhada, em milissegundos. Ver `givePlayoutRoom`. */
const PLAYOUT_TARGET_MS = 250;

/** Por quanto tempo uma leitura de `getStats` vale para quem perguntar de novo. */
const STATS_TTL_MS = 500;

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
        this.sendTransport = null;
        this.sendTransportPromise = null;
        this.producers = new Map();
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
        this.statsReports = [];
        this.statsAt = 0;
    }

    emit(name, detail) {
        this.dispatchEvent(new CustomEvent(name, { detail }));
    }

    /**
     * `identity` e `{ room, name, installId }`: o codigo da sala, como voce aparece para
     * os outros, e qual instalacao do app e esta. O ultimo e o que sustenta a posse da
     * sala do outro lado — ele sobrevive a reconectar, e o `peerId` nao.
     *
     * Pode ser uma função que devolve (a promessa d)esse objeto: o canal de voz entra com
     * um token que vale 60 s, então ele é pedido de novo antes de CADA `join`, inclusive
     * nas reconexões. A sala anônima continua passando o objeto puro.
     *
     * O token é pedido junto com a abertura do socket, e não depois: são duas idas ao
     * servidor que não dependem uma da outra.
     */
    connect(url, identity) {
        this.url = url;
        this.identity = identity;
        this.closedByUs = false;

        return Promise.all([this.openSocket(), this.resolveIdentity()]).then(([, resolved]) => this.setup(resolved));
    }

    resolveIdentity() {
        return typeof this.identity === 'function' ? this.identity() : Promise.resolve(this.identity);
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
        // `closedByUs` também cobre o `disconnect()` chamado de dentro do `identity()`:
        // o token foi recusado, e insistir seria bater na mesma porta a cada segundo.
        if (this.reconnectTimer || this.closedByUs) {
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
            this.emit('reconnected', { resumed: joined.resumed, peers: joined.peers ?? [], can: joined.can ?? null });
        } catch (error) {
            this.emit('reconnecting', { attempt: this.reconnectAttempt, error: error.message });
            this.scheduleReconnect();
        }
    }

    handleMessage(message) {
        if (message.event) {
            this.emit('diagnostic', { event: `sfu.${message.event}`, data: message.data });

            // Outra janela tomou o lugar desta, ou o servidor a expulsou: reconectar
            // seria voltar a brigar pela mesma vaga.
            if (message.event === 'replaced' || message.event === 'kicked') {
                this.closedByUs = true;
            }

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

    /** Só o que mexe em `peers` avisa `peersChanged`: quem escuta redesenha a lista. */
    trackPeers(event, data) {
        let changed = false;

        if (event === 'peerJoined') {
            this.peers.set(data.peerId, { peerId: data.peerId, userId: data.userId, name: data.name, sharing: false, producers: [] });
            changed = true;
        }

        if (event === 'peerLeft') {
            changed = this.peers.delete(data.peerId);
        }

        if (event === 'peerConnectionLost' || event === 'peerReconnected') {
            const peer = this.peers.get(data.peerId);

            if (peer) {
                peer.reconnecting = event === 'peerConnectionLost';
                changed = true;
            }
        }

        if (event === 'newProducer') {
            changed = this.trackProducer(data.peerId, data);
        }

        if (event === 'producerClosed') {
            changed = this.forgetProducer(data.peerId, data.producerId);
        }

        if (event === 'producerPaused' || event === 'producerResumed') {
            const producer = this.peers.get(data.peerId)?.producers.find(item => item.producerId === data.producerId);

            if (producer) {
                producer.paused = event === 'producerPaused';
                changed = true;
            }
        }

        if (event === 'consumerClosed') {
            this.consumers.get(data.consumerId)?.close();
            this.consumers.delete(data.consumerId);
            this.consumerPeers.delete(data.consumerId);
        }

        if (changed) {
            this.emit('peersChanged', [...this.peers.entries()]);
        }
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
            return false;
        }

        peer.producers = [...(peer.producers ?? []).filter(item => item.producerId !== producerId), { producerId, kind, source }];
        peer.sharing = peer.producers.some(item => item.source === 'screen');

        return true;
    }

    forgetProducer(peerId, producerId) {
        const peer = this.peers.get(peerId);

        if (! peer) {
            return false;
        }

        peer.producers = (peer.producers ?? []).filter(item => item.producerId !== producerId);
        peer.sharing = peer.producers.some(item => item.source === 'screen');

        return true;
    }

    /**
     * Os consumers que carregam a midia de uma pessoa — o que pausar quando ninguem quer
     * ver. `kind` separa tela de mic: sem ele, pausar uma calava a outra.
     */
    consumersOf(peerId, kind = null) {
        return [...this.consumerPeers]
            .filter(([consumerId, owner]) => owner === peerId && (! kind || this.consumers.get(consumerId)?.kind === kind))
            .map(([consumerId]) => consumerId);
    }

    /**
     * Pausa no servidor, nao so no elemento `<video>`.
     *
     * Parar o video sozinho continuaria baixando e decodificando tudo: o custo que
     * incomoda quem so quer ouvir esta no decoder, e ele so para quando o pacote deixa
     * de chegar. `pauseConsumer` e o unico jeito de o pacote deixar de chegar.
     */
    async setPeerPaused(peerId, paused, kind = null) {
        const action = paused ? 'pauseConsumer' : 'resumeConsumer';

        await Promise.all(this.consumersOf(peerId, kind).map(consumerId => this.request(action, { consumerId })));
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

    async setup(identity = null) {
        // A `resumeKey` e o que prova ser a mesma pessoa depois de uma queda. Ela so vale
        // com `resume`, e so pedimos resume se o transporte desta sessao continua vivo:
        // reaberto o app, ele e novo em folha e retomar deixaria o cliente sem transporte.
        const joined = await this.request('join', {
            ...(identity ?? await this.resolveIdentity()),
            resumeKey: this.resumeKey,
            resume: Boolean(this.recvTransport),
        });

        this.peerId = joined.peerId;
        this.resumeKey = joined.resumeKey;

        // Retomada: transportes, producers e consumers do servidor seguem ativos.
        if (joined.resumed) {
            return joined;
        }

        // Sessão nova: o que era da antiga morreu no servidor. Fechar aqui também, senão
        // cada decoder da sessão velha segue vivo até o app fechar.
        this.closeMedia();
        // Uma abertura que falhou pode reconectar com outro id. Remonta a lista de
        // participantes pelo retrato do servidor em vez de guardar sessão velha.
        this.peers.clear();
        this.peerLatency.clear();
        this.peers.set(joined.peerId, { peerId: joined.peerId, name: joined.name, self: true, sharing: false, producers: [] });

        for (const peer of joined.peers) {
            this.peers.set(peer.peerId, {
                peerId: peer.peerId,
                userId: peer.userId,
                name: peer.name,
                producers: peer.producers,
                sharing: peer.producers.some(producer => producer.source === 'screen'),
            });
        }

        // Sem WebRTC no motor da janela (WebKitGTK compilado sem ele, como no Parrot), a
        // sala continua funcionando: criar, entrar e transmitir não passam por WebRTC —
        // a tela sobe como RTP puro pelo Rust. Só assistir fica de fora.
        if (typeof RTCPeerConnection === 'undefined') {
            this.emit('diagnostic', { event: 'device.unsupported', data: { userAgent: navigator.userAgent } });

            return joined;
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

    /** Transportes, producers e consumers desta sessão: o que morre com ela. */
    closeMedia() {
        for (const consumer of this.consumers.values()) {
            consumer.close();
        }

        this.consumers.clear();
        this.consumerPeers.clear();
        this.recvTransport?.close();
        this.recvTransport = null;
        // Quem publica escuta `reconnected` e publica de novo.
        this.sendTransport?.close();
        this.sendTransport = null;
        this.sendTransportPromise = null;
        this.producers.clear();
    }

    /**
     * O de recepcao. A tela nao passa por aqui: ela sobe como RTP puro, direto do Rust
     * para a porta que o servidor devolve no `producePlain`. Mic e camera (Windows/macOS)
     * sobem pelo `createSendTransport`.
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

    /** O de envio, criado na primeira publicação: quem só assiste nunca paga por ele. */
    async createSendTransport() {
        const params = await this.request('createTransport');
        const transport = this.device.createSendTransport({
            id: params.transportId,
            iceParameters: params.iceParameters,
            iceCandidates: params.iceCandidates,
            dtlsParameters: params.dtlsParameters,
        });

        transport.on('connect', ({ dtlsParameters }, callback, errback) =>
            this.request('connectTransport', { transportId: transport.id, dtlsParameters })
                .then(callback)
                .catch(errback));

        transport.on('produce', ({ kind, rtpParameters, appData }, callback, errback) =>
            this.request('produce', { transportId: transport.id, kind, source: appData.source, rtpParameters })
                .then(({ producerId }) => callback({ id: producerId }))
                .catch(errback));

        return transport;
    }

    /**
     * `source` e `mic` ou `camera`; `options` vai inteiro para o `produce` do mediasoup
     * (`codecOptions`, por exemplo). Devolve o producer do mediasoup-client.
     *
     * A promessa do transporte é guardada, não só o transporte: mic e câmera publicados
     * ao mesmo tempo criariam dois, e o segundo ficaria órfão no servidor.
     */
    async produce(track, source, options = {}) {
        if (! this.device) {
            throw new Error('esta máquina não tem WebRTC no motor da janela');
        }

        this.sendTransportPromise ??= this.createSendTransport().catch(failure => {
            this.sendTransportPromise = null;
            throw failure;
        });
        this.sendTransport = await this.sendTransportPromise;

        const producer = await this.sendTransport.produce({ track, appData: { source }, ...options });

        this.producers.set(producer.id, producer);
        producer.on('transportclose', () => this.producers.delete(producer.id));

        return producer;
    }

    /** Pausa no servidor E aqui: só aqui continuaria subindo silêncio codificado. */
    async pauseProducer(producerId) {
        await this.request('pauseProducer', { producerId });
        this.producers.get(producerId)?.pause();
    }

    async resumeProducer(producerId) {
        await this.request('resumeProducer', { producerId });
        this.producers.get(producerId)?.resume();
    }

    /** Um pedido que pode falhar sem travar quem chamou: a falha vai para o diagnóstico. */
    tolerate(action, data) {
        return this.request(action, data).catch(failure => {
            this.emit('diagnostic', { event: `sfu.${action}.error`, data: { message: failure.message ?? String(failure) } });

            return null;
        });
    }

    async closeProducer(producerId) {
        await this.tolerate('closeProducer', { producerId });
        this.producers.get(producerId)?.close();
        this.producers.delete(producerId);
    }

    async closeConsumer(consumerId) {
        await this.tolerate('closeConsumer', { consumerId });
        this.consumers.get(consumerId)?.close();
        this.consumers.delete(consumerId);
        this.consumerPeers.delete(consumerId);
    }

    canWatch() {
        return this.recvTransport !== null;
    }

    async consume(producerId) {
        if (! this.recvTransport) {
            throw new Error('esta máquina não tem WebRTC no motor da janela: dá para transmitir, mas não para assistir');
        }

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
        this.givePlayoutRoom(consumer, params.source);
        await this.request('resumeConsumer', { consumerId: consumer.id });

        return { consumer, ...params };
    }

    /**
     * Folga de reprodução da tela, para a retransmissão ter tempo de chegar.
     *
     * O servidor reenvia pacote perdido quando o player pede, mas o pedido leva uma ida e
     * volta — daqui até os EUA são uns 150 ms. Com a folga padrão, o pacote reenviado chega
     * depois da hora de exibir e é jogado fora: a imagem congelou do mesmo jeito.
     *
     * Só a tela: câmera e áudio são conversa, e atraso em conversa é o que se nota
     * primeiro. O nome da propriedade mudou entre versões do motor, então os dois são
     * tentados e a falta de ambos só custa a folga.
     */
    givePlayoutRoom(consumer, source) {
        const receiver = consumer.rtpReceiver;

        if (source !== 'screen' || ! receiver) {
            return;
        }

        try {
            receiver.jitterBufferTarget = PLAYOUT_TARGET_MS;
        } catch {
            try {
                receiver.playoutDelayHint = PLAYOUT_TARGET_MS / 1000;
            } catch (failure) {
                this.emit('diagnostic', { event: 'sfu.playout.error', data: { message: failure.message ?? String(failure) } });
            }
        }
    }

    consumersHasProducer(producerId) {
        return [...this.consumers.values()].some(consumer => consumer.producerId === producerId);
    }

    /** Os relatórios do `getStats`, guardados meio segundo: o ping e cada legenda leem os mesmos. */
    async stats() {
        if (! this.recvTransport) {
            return [];
        }

        if (performance.now() - this.statsAt > STATS_TTL_MS) {
            this.statsReports = [...(await this.recvTransport.getStats()).values()];
            this.statsAt = performance.now();
        }

        return this.statsReports;
    }

    async updatePeerLatency() {
        if (! this.recvTransport) {
            return;
        }

        const reports = await this.stats();
        const transport = reports.find(report =>
            report.type === 'candidate-pair' && report.state === 'succeeded' && report.nominated);
        const rtt = transport?.currentRoundTripTime;

        this.transportRttMs = rtt != null ? Math.round(rtt * 1000) : null;

        // Sozinho na sala, ou antes do primeiro consumer, o transporte ainda não tem par
        // de candidatos com estatística — e a lista mostrava `-- ms` para todo mundo,
        // como se a rede estivesse morta. A ida e volta da sinalização é uma medida
        // pior, mas é uma medida.
        const latency = this.transportRttMs ?? this.lastRttMs;

        for (const peerId of this.peers.keys()) {
            this.peerLatency.set(peerId, latency);
        }

        for (const [consumerId, peerId] of this.consumerPeers) {
            const ssrc = this.consumers.get(consumerId)?.rtpParameters?.encodings?.[0]?.ssrc;
            const jitter = reports.find(report => report.type === 'inbound-rtp' && report.ssrc === ssrc)?.jitter;

            this.peerLatency.set(peerId, rtt != null ? Math.round(rtt * 1000) : jitter != null ? Math.round(jitter * 2000) : null);
        }
    }

    /** Sair de propósito: sem isto o servidor trata como queda e a pessoa vira fantasma. */
    async leaveRoom() {
        await this.tolerate('leave');
    }

    disconnect() {
        this.closedByUs = true;
        clearTimeout(this.reconnectTimer);
        this.reconnectTimer = null;
        this.socketGeneration += 1;
        this.socket?.close();
        this.closeMedia();
        this.peerLatency.clear();
    }
}
