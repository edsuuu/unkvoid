import { Device } from 'mediasoup-client';

// minFrameRate é o piso pedido à CAPTURA: a fonte não entrega menos que isso, então
// o encoder nunca cai para os 5 fps. Se a banda apertar, quem cede é a resolução.
const PROFILES = {
    720: { width: 1280, height: 720, frameRate: 60, minFrameRate: 30, bitrate: 4_000_000 },
    1080: { width: 1920, height: 1080, frameRate: 60, minFrameRate: 30, bitrate: 7_000_000 },
    1440: { width: 2560, height: 1440, frameRate: 60, minFrameRate: 30, bitrate: 12_000_000 },
};

export class SfuClient extends EventTarget {
    constructor() {
        super();
        this.socket = null;
        this.device = null;
        this.sendTransport = null;
        this.recvTransport = null;
        this.pending = new Map();
        this.nextRequestId = 1;
        this.producers = new Map();
        this.consumers = new Map();
        this.peerId = null;
        this.role = 'member';
        this.peers = new Map();
        this.tokenProvider = null;
        this.url = null;
        this.closedByUs = false;
        this.reconnectAttempt = 0;
        this.reconnectTimer = null;
        this.localTracks = new Map();
    }

    emit(name, detail) {
        this.dispatchEvent(new CustomEvent(name, { detail }));
    }

    connect(url, tokenProvider) {
        this.url = url;
        this.tokenProvider = tokenProvider;
        this.closedByUs = false;

        return this.openSocket().then(() => this.setup());
    }

    openSocket() {
        return new Promise((resolve, reject) => {
            this.socket = new WebSocket(this.url);
            this.socket.onerror = () => reject(new Error('não foi possível abrir o WebSocket'));
            this.socket.onmessage = message => this.handleMessage(JSON.parse(message.data));
            this.socket.onclose = () => this.handleClose();
            this.socket.onopen = () => resolve();
        });
    }

    /**
     * A mídia WebRTC não cai junto com a sinalização. Então uma queda de socket é
     * tratada como soluço: tenta voltar com backoff e, se o servidor ainda tiver a
     * sessão, ninguém percebe. Se não tiver, republica tudo do zero.
     */
    handleClose() {
        for (const waiting of this.pending.values()) {
            waiting.reject(new Error('conexão caiu'));
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
            this.peers.set(data.peerId, { name: data.name, avatar: data.avatar, sharing: false });
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

        if (event === 'producerClosed' || event === 'peerProducersClosed') {
            this.markSharing(data.peerId, false);
        }

        this.emit('peersChanged', [...this.peers.entries()]);
    }

    markSharing(peerId, sharing) {
        const peer = this.peers.get(peerId);

        if (peer) {
            peer.sharing = sharing;
        }
    }

    request(action, data = {}) {
        const id = this.nextRequestId++;

        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.socket.send(JSON.stringify({ id, action, data }));
        });
    }

    async setup() {
        // Só pede retomada se os transports desta aba ainda estiverem vivos. Depois
        // de um F5 eles não existem, então a sessão precisa nascer limpa.
        const joined = await this.request('join', {
            token: await this.tokenProvider(),
            resume: Boolean(this.sendTransport && this.recvTransport),
        });

        this.peerId = joined.peerId;
        this.role = joined.role;

        // Retomada: transports, producers e consumers do servidor seguem de pé.
        if (joined.resumed) {
            return joined;
        }

        this.producers.clear();
        this.consumers.clear();
        this.peers.set(joined.peerId, { name: joined.name, avatar: null, self: true, sharing: false });

        for (const peer of joined.peers) {
            this.peers.set(peer.peerId, {
                name: peer.name,
                avatar: peer.avatar,
                sharing: peer.producers.some(producer => producer.source === 'screen'),
            });
        }
        this.device = new Device();
        await this.device.load({ routerRtpCapabilities: joined.routerRtpCapabilities });

        this.sendTransport = await this.createTransport('send');
        this.recvTransport = await this.createTransport('recv');

        await this.republishLocalTracks();

        return joined;
    }

    async republishLocalTracks() {
        for (const [source, entry] of this.localTracks) {
            if (entry.track.readyState !== 'live') {
                this.localTracks.delete(source);

                continue;
            }

            const producer = await this.sendTransport.produce({
                track: entry.track,
                ...entry.options,
                appData: { source },
            });

            this.producers.set(source, producer);
        }

        if (this.localTracks.has('screen')) {
            this.markSharing(this.peerId, true);
            this.emit('peersChanged', [...this.peers.entries()]);
        }
    }

    async createTransport(direction) {
        const params = await this.request('createTransport');
        const options = {
            id: params.transportId,
            iceParameters: params.iceParameters,
            iceCandidates: params.iceCandidates,
            dtlsParameters: params.dtlsParameters,
        };

        const transport = direction === 'send'
            ? this.device.createSendTransport(options)
            : this.device.createRecvTransport(options);

        transport.on('connect', ({ dtlsParameters }, callback, errback) =>
            this.request('connectTransport', { transportId: transport.id, dtlsParameters })
                .then(callback)
                .catch(errback));

        if (direction === 'send') {
            transport.on('produce', ({ kind, rtpParameters, appData }, callback, errback) =>
                this.request('produce', {
                    transportId: transport.id,
                    kind,
                    rtpParameters,
                    source: appData.source,
                })
                    .then(({ producerId }) => callback({ id: producerId }))
                    .catch(errback));
        }

        return transport;
    }

    async shareScreen({ profile, codec, simulcast, contentHint }) {
        const preset = PROFILES[profile];
        const stream = await navigator.mediaDevices.getDisplayMedia({
            video: {
                width: { ideal: preset.width },
                height: { ideal: preset.height },
                frameRate: { min: preset.minFrameRate, ideal: preset.frameRate },
            },
            audio: true,
            systemAudio: 'include',
            selfBrowserSurface: 'exclude',
            surfaceSwitching: 'include',
        });

        const videoTrack = stream.getVideoTracks()[0];
        videoTrack.contentHint = contentHint;
        videoTrack.addEventListener('ended', () => this.emit('shareEnded'));

        const publishOptions = {
            encodings: this.buildEncodings(preset, codec, simulcast),
            codecOptions: { videoGoogleStartBitrate: Math.round(preset.bitrate / 2000) },
            codec: this.pickCodec(codec),
            // Perder nitidez é melhor que engasgar: mantém o FPS estável.
            degradationPreference: 'maintain-framerate',
        };

        const video = await this.sendTransport.produce({
            track: videoTrack,
            ...publishOptions,
            appData: { source: 'screen' },
        });

        this.producers.set('screen', video);
        this.localTracks.set('screen', { track: videoTrack, options: publishOptions });
        this.markSharing(this.peerId, true);
        this.emit('peersChanged', [...this.peers.entries()]);

        const audioTrack = stream.getAudioTracks()[0];

        if (audioTrack) {
            const audio = await this.sendTransport.produce({
                track: audioTrack,
                appData: { source: 'screenAudio' },
            });

            this.producers.set('screenAudio', audio);
            this.localTracks.set('screenAudio', { track: audioTrack, options: {} });
        }

        return { hasAudio: Boolean(audioTrack), track: videoTrack };
    }

    /**
     * Troca a qualidade sem parar de compartilhar: reconfigura a captura e o encoder
     * no lugar, sem republicar o track (republicar faria a tela piscar para todo mundo).
     */
    async changeQuality(profile) {
        const producer = this.producers.get('screen');
        const preset = PROFILES[profile];

        if (!producer || !preset) {
            return false;
        }

        await producer.track.applyConstraints({
            width: { ideal: preset.width },
            height: { ideal: preset.height },
            frameRate: { min: preset.minFrameRate, ideal: preset.frameRate },
        });

        const sender = producer.rtpSender;

        if (!sender) {
            return true;
        }

        const parameters = sender.getParameters();

        for (const encoding of parameters.encodings ?? []) {
            encoding.maxBitrate = preset.bitrate;
            encoding.maxFramerate = preset.frameRate;
        }

        await sender.setParameters(parameters);

        return true;
    }

    buildEncodings(preset, codec, simulcast) {
        if (codec === 'vp9' || codec === 'av1') {
            return [{ maxBitrate: preset.bitrate, scalabilityMode: 'L3T3_KEY' }];
        }

        if (!simulcast) {
            return [{ maxBitrate: preset.bitrate, maxFramerate: preset.frameRate }];
        }

        return [
            { rid: 'low', maxBitrate: 500_000, scaleResolutionDownBy: 4, maxFramerate: 15 },
            { rid: 'mid', maxBitrate: 1_500_000, scaleResolutionDownBy: 2, maxFramerate: 30 },
            { rid: 'high', maxBitrate: preset.bitrate, scaleResolutionDownBy: 1, maxFramerate: preset.frameRate },
        ];
    }

    pickCodec(codec) {
        return this.device.rtpCapabilities.codecs
            .find(candidate => candidate.mimeType.toLowerCase() === `video/${codec}`);
    }

    async stopShare() {
        for (const source of ['screen', 'screenAudio']) {
            const producer = this.producers.get(source);

            if (!producer) {
                continue;
            }

            producer.track?.stop();
            producer.close();
            await this.request('closeProducer', { producerId: producer.id }).catch(() => {});
            this.producers.delete(source);
            this.localTracks.delete(source);
        }

        this.markSharing(this.peerId, false);
        this.emit('peersChanged', [...this.peers.entries()]);
    }

    async toggleMicrophone() {
        const existing = this.producers.get('mic');

        if (existing) {
            existing.track.stop();
            existing.close();
            await this.request('closeProducer', { producerId: existing.id }).catch(() => {});
            this.producers.delete('mic');
            this.localTracks.delete('mic');

            return false;
        }

        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        const producer = await this.sendTransport.produce({
            track: stream.getAudioTracks()[0],
            appData: { source: 'mic' },
        });

        this.producers.set('mic', producer);
        this.localTracks.set('mic', { track: stream.getAudioTracks()[0], options: {} });

        return true;
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

    pauseConsumer(consumerId) {
        return this.request('pauseConsumer', { consumerId });
    }

    resumeConsumerById(consumerId) {
        return this.request('resumeConsumer', { consumerId });
    }

    stopBroadcastOf(peerId) {
        return this.request('stopBroadcast', { peerId });
    }

    disconnectPeer(peerId) {
        return this.request('disconnectPeer', { peerId });
    }

    async outboundStats() {
        const producer = this.producers.get('screen');

        if (!producer) {
            return null;
        }

        const rows = [];
        const stats = await producer.getStats();

        stats.forEach(report => {
            if (report.type !== 'outbound-rtp') {
                return;
            }

            rows.push({
                layer: report.rid ?? 'única',
                resolution: `${report.frameWidth ?? '?'}×${report.frameHeight ?? '?'}`,
                fps: report.framesPerSecond ?? 0,
                limitedBy: report.qualityLimitationReason ?? 'none',
                encoder: report.encoderImplementation ?? '?',
                encodeMs: report.framesEncoded
                    ? (report.totalEncodeTime / report.framesEncoded * 1000).toFixed(2)
                    : '—',
                bytesSent: report.bytesSent ?? 0,
                id: report.id,
            });
        });

        return rows;
    }

    /** Saída explícita: sem isto o servidor trata como queda e a pessoa fica fantasma. */
    async leaveRoom() {
        await this.request('leave').catch(() => {});
    }

    disconnect() {
        this.closedByUs = true;
        clearTimeout(this.reconnectTimer);
        this.socket?.close();
        this.sendTransport?.close();
        this.recvTransport?.close();
        this.producers.clear();
        this.consumers.clear();
        this.localTracks.clear();
    }
}
