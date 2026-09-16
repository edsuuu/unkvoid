import { Device } from 'mediasoup-client';
import type {
    Consumer,
    DtlsParameters,
    IceCandidate,
    IceParameters,
    MediaKind,
    Producer,
    ProducerOptions,
    RtpCapabilities,
    RtpParameters,
    Transport,
} from 'mediasoup-client/types';

import { Failure } from './Failure.ts';

const PLAYOUT_TARGET_MS = 250;

const STATS_TTL_MS = 500;

export type SourceName = 'screen' | 'screenAudio' | 'mic' | 'camera';

export type ProducerInfo = {
    producerId: string;
    kind: MediaKind;
    source: SourceName;
    paused?: boolean;
};

export type PeerDescription = {
    peerId: string;
    userId: string;
    name: string;
    producers: ProducerInfo[];
};

export type SfuPeer = {
    peerId: string;
    userId?: string;
    name: string;
    self?: boolean;
    sharing: boolean;
    producers: ProducerInfo[];
    reconnecting?: boolean;
};

export type RoomIdentity = { room: string; name: string; installId: string } | { token: string };

export type IdentitySource = RoomIdentity | (() => Promise<RoomIdentity>);

export type JoinResponse = {
    resumed: boolean;
    peerId: string;
    name: string;
    resumeKey: string;
    routerRtpCapabilities: RtpCapabilities;
    peers: PeerDescription[];
    userId: string;
    can: string[];
};

export type SfuEventData = {
    peerId: string;
    userId: string;
    name: string | null;
    producerId: string;
    consumerId: string;
    kind: MediaKind;
    source: SourceName;
    muted: boolean;
    reason?: string;
    watchers?: { peerId: string; name: string }[];
};

export type Diagnostic = { event: string; data?: unknown };

export type Reconnecting = { attempt: number; error?: string };

export type Reconnected = { resumed: boolean; peers: PeerDescription[]; can: string[] | null };

export type SrtpParameters = { cryptoSuite: string; keyBase64: string };

export type ConsumeResponse = {
    consumerId: string;
    producerId: string;
    kind: MediaKind;
    rtpParameters: RtpParameters;
    peerId: string;
    name: string;
    source: SourceName;
};

export type PlainProducerResponse = {
    producerId: string;
    kind: MediaKind;
    source: SourceName;
    ip: string;
    port: number;
    srtpParameters?: SrtpParameters | null;
};

export type PlainConsumerResponse = {
    consumerId: string;
    producerId: string;
    kind: MediaKind;
    payloadType: number | null;
    ssrc: number | null;
    ip: string;
    port: number;
    srtpParameters: SrtpParameters;
    peerId: string;
    name: string;
    source: SourceName;
};

export type StatsReport = {
    type: string;
    state?: string;
    nominated?: boolean;
    currentRoundTripTime?: number;
    ssrc?: number;
    jitter?: number;
    packetsLost: number;
    packetsReceived: number;
    bytesReceived: number;
};

type TransportResponse = {
    transportId: string;
    iceParameters: IceParameters;
    iceCandidates: IceCandidate[];
    dtlsParameters: DtlsParameters;
};

type SfuMessage =
    | { event: string; data: SfuEventData }
    | { event?: undefined; id: number; ok: boolean; data?: unknown; error?: string };

type PendingRequest = { resolve: (value: unknown) => void; reject: (reason: unknown) => void };

type PlayoutReceiver = NonNullable<Consumer['rtpReceiver']> & { jitterBufferTarget?: number | null; playoutDelayHint?: number };

export class SfuClient extends EventTarget {
    static readonly REQUEST_TIMEOUT_MS = 10_000;

    socket: WebSocket | null = null;
    device: Device | null = null;
    recvTransport: Transport | null = null;
    sendTransport: Transport | null = null;
    sendTransportPromise: Promise<Transport> | null = null;
    producers = new Map<string, Producer>();
    pending = new Map<number, PendingRequest>();
    nextRequestId = 1;
    consumers = new Map<string, Consumer>();
    consumerPeers = new Map<string, string>();
    peerLatency = new Map<string, number | null>();
    peerId: string | null = null;
    peers = new Map<string, SfuPeer>();
    identity: IdentitySource | null = null;
    resumeKey: string | null = null;
    url: string | null = null;
    videoCodecs: string[] = [];
    closedByUs = false;
    reconnectAttempt = 0;
    reconnectTimer: number | null = null;
    socketGeneration = 0;
    lastRttMs: number | null = null;
    transportRttMs: number | null = null;
    statsReports: StatsReport[] = [];
    statsAt = 0;

    static handler(): { handlerName?: 'Safari12' } {
        const agent = navigator.userAgent;
        const webkitWithoutChrome = /AppleWebKit/i.test(agent) && ! /Chrome|Chromium|Edg/i.test(agent);

        return webkitWithoutChrome && ! /\bSafari\b/i.test(agent) ? { handlerName: 'Safari12' } : {};
    }

    emit(name: string, detail?: unknown): void {
        this.dispatchEvent(new CustomEvent(name, { detail }));
    }

    on(name: 'diagnostic', handler: (detail: Diagnostic) => void): void;
    on(name: 'reconnecting', handler: (detail: Reconnecting) => void): void;
    on(name: 'reconnected', handler: (detail: Reconnected) => void): void;
    on(name: 'peersChanged' | 'closed' | 'replaced', handler: () => void): void;
    on(name: string, handler: (detail: SfuEventData) => void): void;
    on(name: string, handler: (detail: never) => void): void {
        this.addEventListener(name, event => handler((event as CustomEvent<never>).detail));
    }

    connect(url: string, identity: IdentitySource): Promise<JoinResponse> {
        this.url = url;
        this.identity = identity;
        this.closedByUs = false;

        return Promise.all([this.openSocket(), this.resolveIdentity()]).then(([, resolved]) => this.setup(resolved));
    }

    resolveIdentity(): Promise<RoomIdentity> {
        return typeof this.identity === 'function' ? this.identity() : Promise.resolve(this.identity as RoomIdentity);
    }

    openSocket(): Promise<void> {
        return new Promise((resolve, reject) => {
            const generation = ++this.socketGeneration;
            const socket = new WebSocket(this.url ?? '');

            this.socket = socket;
            socket.onerror = () => {
                if (generation === this.socketGeneration) {
                    reject(new Error('unable to open the WebSocket'));
                }
            };
            socket.onmessage = message => {
                if (generation === this.socketGeneration) {
                    this.handleMessage(JSON.parse(message.data) as SfuMessage);
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

    handleClose(): void {
        this.emit('diagnostic', { event: 'socket.close', data: { attempt: this.reconnectAttempt } });
        for (const waiting of this.pending.values()) {
            waiting.reject(new Error('connection dropped'));
        }

        this.pending.clear();

        if (this.closedByUs) {
            return;
        }

        this.emit('reconnecting', { attempt: this.reconnectAttempt + 1 });
        this.scheduleReconnect();
    }

    scheduleReconnect(): void {
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

    async reconnect(): Promise<void> {
        try {
            await this.openSocket();

            const joined = await this.setup();

            this.reconnectAttempt = 0;
            this.emit('reconnected', { resumed: joined.resumed, peers: joined.peers ?? [], can: joined.can ?? null });
        } catch (failure) {
            this.emit('reconnecting', { attempt: this.reconnectAttempt, error: Failure.message(failure) });
            this.scheduleReconnect();
        }
    }

    handleMessage(message: SfuMessage): void {
        if (message.event) {
            this.emit('diagnostic', { event: `sfu.${message.event}`, data: message.data });

            if (message.event === 'replaced' || message.event === 'kicked') {
                this.closedByUs = true;
            }

            if (message.event === 'peerLeft') {
                message.data = { ...message.data, name: this.peers.get(message.data.peerId)?.name ?? null };
            }

            this.trackPeers(message.event, message.data);
            this.emit(message.event, message.data);

            return;
        }

        const reply = message as Extract<SfuMessage, { id: number }>;
        const waiting = this.pending.get(reply.id);

        if (! waiting) {
            return;
        }

        this.pending.delete(reply.id);

        if (reply.ok) {
            waiting.resolve(reply.data);
        } else {
            waiting.reject(new Error(reply.error));
        }
    }

    trackPeers(event: string, data: SfuEventData): void {
        let changed = false;

        if (event === 'peerJoined') {
            this.peers.set(data.peerId, { peerId: data.peerId, userId: data.userId, name: data.name ?? '', sharing: false, producers: [] });
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

    trackProducer(peerId: string, { producerId, kind, source }: ProducerInfo): boolean {
        const peer = this.peers.get(peerId);

        if (! peer) {
            return false;
        }

        peer.producers = [...(peer.producers ?? []).filter(item => item.producerId !== producerId), { producerId, kind, source }];
        peer.sharing = peer.producers.some(item => item.source === 'screen');

        return true;
    }

    forgetProducer(peerId: string, producerId: string): boolean {
        const peer = this.peers.get(peerId);

        if (! peer) {
            return false;
        }

        peer.producers = (peer.producers ?? []).filter(item => item.producerId !== producerId);
        peer.sharing = peer.producers.some(item => item.source === 'screen');

        return true;
    }

    consumersOf(peerId: string, kind: MediaKind | null = null): string[] {
        return [...this.consumerPeers]
            .filter(([consumerId, owner]) => owner === peerId && (! kind || this.consumers.get(consumerId)?.kind === kind))
            .map(([consumerId]) => consumerId);
    }

    async setPeerPaused(peerId: string, paused: boolean, kind: MediaKind | null = null): Promise<void> {
        const action = paused ? 'pauseConsumer' : 'resumeConsumer';

        await Promise.all(this.consumersOf(peerId, kind).map(consumerId => this.request(action, { consumerId })));
    }

    request<Result = unknown>(action: string, data: Record<string, unknown> = {}): Promise<Result> {
        const id = this.nextRequestId++;
        const startedAt = performance.now();

        return new Promise<Result>((resolve, reject) => {
            const deadline = setTimeout(() => {
                this.pending.delete(id);
                reject(new Error(`o servidor não respondeu a "${action}"`));
            }, SfuClient.REQUEST_TIMEOUT_MS);

            const settle = (finish: (value: never) => void) => (value: unknown) => {
                clearTimeout(deadline);
                this.lastRttMs = Math.round(performance.now() - startedAt);
                finish(value as never);
            };

            this.pending.set(id, { resolve: settle(resolve), reject: settle(reject) });

            try {
                this.socket!.send(JSON.stringify({ id, action, data }));
            } catch (failure) {
                clearTimeout(deadline);
                this.pending.delete(id);
                reject(failure);
            }
        });
    }

    async setup(identity: RoomIdentity | null = null): Promise<JoinResponse> {
        const joined = await this.request<JoinResponse>('join', {
            ...(identity ?? await this.resolveIdentity()),
            resumeKey: this.resumeKey,
            resume: Boolean(this.recvTransport),
        });

        this.peerId = joined.peerId;
        this.resumeKey = joined.resumeKey;

        if (joined.resumed) {
            return joined;
        }

        this.closeMedia();
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

        if (typeof RTCPeerConnection === 'undefined') {
            this.emit('diagnostic', { event: 'device.unsupported', data: { userAgent: navigator.userAgent } });

            return joined;
        }

        this.device = new Device(SfuClient.handler());
        await this.device.load({ routerRtpCapabilities: joined.routerRtpCapabilities });

        this.videoCodecs = (this.device.rtpCapabilities.codecs ?? [])
            .filter(codec => codec.kind === 'video')
            .map(codec => codec.mimeType);

        this.emit('diagnostic', {
            event: 'device.ready',
            data: { handler: this.device.handlerName, video: this.videoCodecs },
        });

        this.recvTransport = await this.createTransport();

        return joined;
    }

    closeMedia(): void {
        for (const consumer of this.consumers.values()) {
            consumer.close();
        }

        this.consumers.clear();
        this.consumerPeers.clear();
        this.recvTransport?.close();
        this.recvTransport = null;
        this.sendTransport?.close();
        this.sendTransport = null;
        this.sendTransportPromise = null;
        this.producers.clear();
    }

    async createTransport(): Promise<Transport> {
        const params = await this.request<TransportResponse>('createTransport');
        const transport = this.device!.createRecvTransport({
            id: params.transportId,
            iceParameters: params.iceParameters,
            iceCandidates: params.iceCandidates,
            dtlsParameters: params.dtlsParameters,
        });

        transport.on('connect', ({ dtlsParameters }, callback, errback) =>
            this.request('connectTransport', { transportId: transport.id, dtlsParameters })
                .then(callback)
                .catch(errback));

        return transport;
    }

    async createSendTransport(): Promise<Transport> {
        const params = await this.request<TransportResponse>('createTransport');
        const transport = this.device!.createSendTransport({
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
            this.request<{ producerId: string }>('produce', { transportId: transport.id, kind, source: appData.source, rtpParameters })
                .then(({ producerId }) => callback({ id: producerId }))
                .catch(errback));

        return transport;
    }

    async produce(track: MediaStreamTrack, source: SourceName, options: Omit<ProducerOptions, 'track' | 'appData'> = {}): Promise<Producer> {
        if (! this.device) {
            throw new Error('esta máquina não tem WebRTC no motor da janela');
        }

        this.sendTransportPromise ??= this.createSendTransport().catch((failure: unknown) => {
            this.sendTransportPromise = null;
            throw failure;
        });

        const transport = await this.sendTransportPromise;

        this.sendTransport = transport;

        const producer = await transport.produce({ track, appData: { source }, ...options });

        this.producers.set(producer.id, producer);
        producer.on('transportclose', () => this.producers.delete(producer.id));

        return producer;
    }

    async pauseProducer(producerId: string): Promise<void> {
        await this.request('pauseProducer', { producerId });
        this.producers.get(producerId)?.pause();
    }

    async resumeProducer(producerId: string): Promise<void> {
        await this.request('resumeProducer', { producerId });
        this.producers.get(producerId)?.resume();
    }

    tolerate<Result = unknown>(action: string, data?: Record<string, unknown>): Promise<Result | null> {
        return this.request<Result>(action, data).catch((failure: unknown) => {
            this.emit('diagnostic', { event: `sfu.${action}.error`, data: { message: Failure.message(failure) } });

            return null;
        });
    }

    async closeProducer(producerId: string): Promise<void> {
        await this.tolerate('closeProducer', { producerId });
        this.producers.get(producerId)?.close();
        this.producers.delete(producerId);
    }

    async closeConsumer(consumerId: string): Promise<void> {
        await this.tolerate('closeConsumer', { consumerId });
        this.consumers.get(consumerId)?.close();
        this.consumers.delete(consumerId);
        this.consumerPeers.delete(consumerId);
    }

    canWatch(): boolean {
        return this.recvTransport !== null;
    }

    async consume(producerId: string): Promise<ConsumeResponse & { consumer: Consumer }> {
        if (! this.recvTransport) {
            throw new Error('esta máquina não tem WebRTC no motor da janela: dá para transmitir, mas não para assistir');
        }

        const params = await this.request<ConsumeResponse>('consume', {
            transportId: this.recvTransport.id,
            producerId,
            rtpCapabilities: this.device!.rtpCapabilities,
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

    givePlayoutRoom(consumer: Consumer, source: SourceName): void {
        const receiver = consumer.rtpReceiver as PlayoutReceiver | undefined;

        if (source !== 'screen' || ! receiver) {
            return;
        }

        try {
            receiver.jitterBufferTarget = PLAYOUT_TARGET_MS;
        } catch {
            try {
                receiver.playoutDelayHint = PLAYOUT_TARGET_MS / 1000;
            } catch (failure) {
                this.emit('diagnostic', { event: 'sfu.playout.error', data: { message: Failure.message(failure) } });
            }
        }
    }

    consumersHasProducer(producerId: string): boolean {
        return [...this.consumers.values()].some(consumer => consumer.producerId === producerId);
    }

    async stats(): Promise<StatsReport[]> {
        if (! this.recvTransport) {
            return [];
        }

        if (performance.now() - this.statsAt > STATS_TTL_MS) {
            this.statsReports = [...(await this.recvTransport.getStats()).values()] as StatsReport[];
            this.statsAt = performance.now();
        }

        return this.statsReports;
    }

    async updatePeerLatency(): Promise<void> {
        if (! this.recvTransport) {
            return;
        }

        const reports = await this.stats();
        const transport = reports.find(report =>
            report.type === 'candidate-pair' && report.state === 'succeeded' && report.nominated);
        const rtt = transport?.currentRoundTripTime;

        this.transportRttMs = rtt != null ? Math.round(rtt * 1000) : null;

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

    async leaveRoom(): Promise<void> {
        await this.tolerate('leave');
    }

    disconnect(): void {
        this.closedByUs = true;
        clearTimeout(this.reconnectTimer ?? undefined);
        this.reconnectTimer = null;
        this.socketGeneration += 1;
        this.socket?.close();
        this.closeMedia();
        this.peerLatency.clear();
    }
}
