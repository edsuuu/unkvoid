import { Device } from 'mediasoup-client';

// minFrameRate is the floor requested from CAPTURE: the source delivers no less than this, so
// the encoder never drops to 5 fps. If bandwidth is tight, resolution gives way.
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
        // Only request a resume if this tab’s transports are still alive. After
        // F5 they do not exist, so the session must start clean.
        const joined = await this.request('join', {
            token: await this.tokenProvider(),
            resume: Boolean(this.sendTransport && this.recvTransport),
        });

        this.peerId = joined.peerId;
        this.role = joined.role;

        // Resume: the server’s transports, producers, and consumers remain active.
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
            // Losing sharpness is better than stuttering: it keeps FPS stable.
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
     * Changes quality without stopping sharing: reconfigures capture and the encoder
     * in place without republishing the track (republishing would make the screen flicker for everyone).
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

    /**
     * The track comes from outside because who opens the microphone is the gate — it is
     * the gate that decides, frame by frame, whether the audio leaves. Here the producer
     * only stays up; muting never closes it.
     */
    async publishMicrophone(track) {
        if (this.producers.has('mic')) {
            return this.producers.get('mic');
        }

        const producer = await this.sendTransport.produce({
            track,
            appData: { source: 'mic' },
        });

        this.producers.set('mic', producer);
        this.localTracks.set('mic', { track, options: {} });

        return producer;
    }

    /**
     * Relays a message to another participant through the SFU socket. Used by the
     * desktop app to set up its direct connections: the server does not read the
     * payload, it only delivers it, and the sender comes from the session — so the
     * same authenticated room that carries the media also carries the handshake,
     * with no second socket and no second identity.
     */
    async signal(to, kind, payload) {
        return this.request('signal', { to, kind, payload });
    }

    async unpublishMicrophone() {
        const existing = this.producers.get('mic');

        if (! existing) {
            return;
        }

        existing.close();
        await this.request('closeProducer', { producerId: existing.id }).catch(() => {});
        this.producers.delete('mic');
        this.localTracks.delete('mic');
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
                layer: report.rid ?? 'single',
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

    /** Explicit departure: without this, the server treats it as a drop and the person becomes a ghost. */
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
