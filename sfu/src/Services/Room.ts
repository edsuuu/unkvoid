import type {
    PlainTransport,
    Producer,
    Router,
    SrtpParameters,
    WebRtcServer,
    WebRtcTransport,
    Worker,
} from 'mediasoup/types';
import { randomUUID, randomBytes } from 'node:crypto';
import type { WebSocket } from 'ws';

import { Peer, type ProducerDescription } from './Peer.js';
import { Webhook } from './Webhook.js';
import { config } from '../Config/index.js';
import type { SourceName } from '../Enums/Source.js';
import { NotFoundException, ValidationException } from '../Exceptions/ApiException.js';

export type ProducerOwner = { peer: Peer; producer: Producer };

export type JoinOutcome = { peer: Peer; resumed: boolean };

const GRACE_MS = 30_000;

export type PeerDescription = {
    peerId: string;
    userId: string;
    name: string;
    reconnecting: boolean;
    producers: ProducerDescription[];
};

export class Room {
    public readonly peers = new Map<string, Peer>();

    public onEvicted: ((room: Room) => void) | null = null;

    private readonly evictions = new Map<string, NodeJS.Timeout>();

    public constructor(
        public readonly id: string,
        public readonly router: Router,
        private readonly webRtcServer: WebRtcServer,
    ) {}

    public static async create(
        worker: Worker,
        webRtcServer: WebRtcServer,
        id: string,
    ): Promise<Room> {
        const router = await worker.createRouter({ mediaCodecs: config.router.mediaCodecs });

        return new Room(id, router, webRtcServer);
    }

    public addPeer(
        name: string,
        socket: WebSocket,
        identity: { userId: string; can: string[]; ip: string },
        options: { resumeKey?: string | null; resume?: boolean } = {},
    ): JoinOutcome {
        const previous = options.resumeKey ? this.findByResumeKey(options.resumeKey) : null;

        const tokened = !identity.userId.startsWith('guest:');

        const stranger = tokened && previous?.userId !== identity.userId;

        if (previous && options.resume && !stranger) {
            const staleSocket = previous.isOrphaned() ? null : previous.socket;

            this.cancelEviction(previous.id);
            previous.attachSocket(socket);

            if (!staleSocket) {
                this.broadcast('peerReconnected', { peerId: previous.id }, previous.id);
            }

            if (tokened) {
                this.applyCan(previous, identity.can);
            }

            for (const producerId of new Set(
                [...previous.consumers.values()].map((consumer) => consumer.producerId),
            )) {
                this.announceWatchers(producerId);
            }

            if (staleSocket) {
                console.log(
                    `[INFO] socket swapped room=${this.id} sub=${previous.userId} peer=${previous.id} ip=${previous.ip}`,
                );

                staleSocket.terminate();
            }

            return { peer: previous, resumed: true };
        }

        const peer = new Peer(
            randomUUID(),
            name,
            socket,
            randomBytes(16).toString('hex'),
            identity.userId,
            identity.can,
            identity.ip,
        );

        this.peers.set(peer.id, peer);

        if (previous) {
            this.replacePeer(previous);
        }

        return { peer, resumed: false };
    }

    public replacePeer(previous: Peer): void {
        console.log(
            `[INFO] replaced room=${this.id} sub=${previous.userId} peer=${previous.id} ip=${previous.ip}`,
        );
        this.cancelEviction(previous.id);
        previous.send('replaced', { reason: 'you joined again from another connection' });
        previous.socket.close(4002, 'replaced');
        this.removePeer(previous);
    }

    public kickUser(userId: string): number {
        let kicked = 0;

        for (const peer of [...this.peers.values()]) {
            if (peer.userId !== userId) {
                continue;
            }

            this.broadcast('peerKicked', { peerId: peer.id, name: peer.name }, peer.id);
            peer.send('kicked', { reason: 'você foi removido desta sala' });

            peer.socket.close(4001, 'kicked');
            this.removePeer(peer);
            kicked += 1;
        }

        return kicked;
    }

    public async muteUser(userId: string, muted: boolean): Promise<number> {
        let touched = 0;

        for (const peer of this.peers.values()) {
            if (peer.userId !== userId) {
                continue;
            }

            peer.serverMuted = muted;
            peer.send('serverMuted', { muted });

            for (const producer of peer.producers.values()) {
                if (producer.appData.source === 'mic') {
                    await this.setProducerPaused(peer, producer, muted);
                    touched += 1;
                }
            }
        }

        return touched;
    }

    public async setProducerPaused(peer: Peer, producer: Producer, paused: boolean): Promise<void> {
        if (!paused) {
            peer.assertNotServerMuted(String(producer.appData.source));
        }

        await (paused ? producer.pause() : producer.resume());

        this.broadcast(
            paused ? 'producerPaused' : 'producerResumed',
            { peerId: peer.id, producerId: producer.id },
            peer.id,
        );
    }

    private applyCan(peer: Peer, can: string[]): void {
        peer.can = can;

        for (const producer of [...peer.producers.values()]) {
            const source = String(producer.appData.source) as SourceName;

            if (peer.allows(source)) {
                continue;
            }

            console.log(
                `[INFO] revoked room=${this.id} sub=${peer.userId} source=${source} peer=${peer.id} ip=${peer.ip}`,
            );

            if (source === 'screen' || source === 'screenAudio') {
                peer.send('producerDead', {
                    producerId: producer.id,
                    kind: producer.kind,
                    source,
                    reason: 'revoked',
                });
            }

            this.closeProducer(peer, producer);
        }
    }

    public closeProducer(peer: Peer, producer: Producer | undefined): void {
        if (!producer || peer.producers.get(producer.id) !== producer) {
            return;
        }

        producer.close();
        peer.producers.delete(producer.id);
        this.broadcast(
            'producerClosed',
            {
                peerId: peer.id,
                producerId: producer.id,
                kind: producer.kind,
                source: String(producer.appData.source),
            },
            peer.id,
        );

        if (![...peer.producers.values()].some((other) => other.appData.plain === true)) {
            peer.closePlainTransports();
        }
    }

    private findByResumeKey(resumeKey: string): Peer | null {
        return [...this.peers.values()].find((peer) => peer.resumeKey === resumeKey) ?? null;
    }

    public orphanPeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.orphanedAt = Date.now();

        this.broadcast('peerConnectionLost', { peerId: peer.id }, peer.id);

        for (const producerId of new Set(
            [...peer.consumers.values()].map((consumer) => consumer.producerId),
        )) {
            this.announceWatchers(producerId);
        }

        this.evictions.set(
            peer.id,
            setTimeout(() => {
                this.evictions.delete(peer.id);

                if (this.peers.get(peer.id) === peer && peer.isOrphaned()) {
                    this.removePeer(peer);
                }
            }, GRACE_MS),
        );
    }

    private cancelEviction(peerId: string): void {
        const timer = this.evictions.get(peerId);

        if (timer) {
            clearTimeout(timer);
            this.evictions.delete(peerId);
        }
    }

    public activeCount(): number {
        return [...this.peers.values()].filter((peer) => !peer.isOrphaned()).length;
    }

    public findPeer(peerId: string): Peer {
        const peer = this.peers.get(peerId);

        if (!peer) {
            throw new NotFoundException(`participant ${peerId} is not in this room`);
        }

        return peer;
    }

    public removePeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        console.log(`[INFO] left room=${this.id} sub=${peer.userId} peer=${peer.id} ip=${peer.ip}`);
        peer.close();
        this.peers.delete(peer.id);
        this.broadcast('peerLeft', { peerId: peer.id }, peer.id);

        if (![...this.peers.values()].some((other) => other.userId === peer.userId)) {
            Webhook.send('left', this.id, peer);
        }

        this.onEvicted?.(this);
    }

    public describePeers(exceptPeerId?: string, withOrphans = false): PeerDescription[] {
        return [...this.peers.values()]
            .filter((peer) => peer.id !== exceptPeerId && (withOrphans || !peer.isOrphaned()))
            .map((peer) => ({
                peerId: peer.id,
                userId: peer.userId,
                name: peer.name,
                reconnecting: peer.isOrphaned(),
                producers: peer.describeProducers(),
            }));
    }

    public async createTransport(peer: Peer): Promise<WebRtcTransport> {
        const transport = await this.router.createWebRtcTransport({
            webRtcServer: this.webRtcServer,
            enableUdp: config.transport.enableUdp,
            enableTcp: config.transport.enableTcp,
            preferUdp: config.transport.preferUdp,
            initialAvailableOutgoingBitrate: config.transport.initialAvailableOutgoingBitrate,
        });

        await transport.setMaxIncomingBitrate(config.transport.maxIncomingBitrate);

        transport.on('dtlsstatechange', (state) => {
            if (state === 'closed') {
                transport.close();
            }
        });

        peer.addTransport(transport);

        return transport;
    }

    public async plainTransportFor(
        peer: Peer,
        srtpParameters: SrtpParameters,
    ): Promise<PlainTransport> {
        return this.plainTransport(peer, srtpParameters, false);
    }

    public async plainReceiveTransportFor(
        peer: Peer,
        srtpParameters: SrtpParameters,
    ): Promise<PlainTransport> {
        return this.plainTransport(peer, srtpParameters, true);
    }

    private async plainTransport(
        peer: Peer,
        srtpParameters: SrtpParameters,
        receive: boolean,
    ): Promise<PlainTransport> {
        const existing = [...peer.plainTransports.values()].find(
            (transport) => Boolean(transport.appData.receive) === receive,
        );

        if (existing) {
            return existing;
        }

        const transport = await this.router
            .createPlainTransport({
                listenInfo: {
                    protocol: 'udp',
                    ip: '0.0.0.0',
                    announcedAddress: config.announcedAddress,
                },
                rtcpMux: true,
                comedia: true,
                enableSrtp: true,
                srtpCryptoSuite: srtpParameters.cryptoSuite,
                appData: { receive },
            })
            .catch((failure) => {
                throw /no more available ports/i.test(String(failure))
                    ? new ValidationException(
                          'o servidor já está no limite de participantes por sala — tente de novo quando alguém sair',
                      )
                    : failure;
            });

        await transport.connect({ srtpParameters });

        peer.addPlainTransport(transport);

        return transport;
    }

    public announceWatchers(producerId: string): void {
        const owner = [...this.peers.values()].find((peer) => peer.producers.has(producerId));

        if (String(owner?.producers.get(producerId)?.appData.source) !== 'screen') {
            return;
        }

        const watchers = [...this.peers.values()]
            .filter(
                (peer) =>
                    !peer.isOrphaned() &&
                    [...peer.consumers.values()].some(
                        (consumer) => consumer.producerId === producerId && !consumer.paused,
                    ),
            )
            .map((peer) => ({ peerId: peer.id, name: peer.name }));

        this.broadcast('watchers', { producerId, watchers });
    }

    public findProducerOwner(producerId: string): ProducerOwner {
        for (const peer of this.peers.values()) {
            const producer = peer.producers.get(producerId);

            if (producer) {
                return { peer, producer };
            }
        }

        throw new NotFoundException(`producer ${producerId} does not exist in this room`);
    }

    public broadcast(event: string, data: unknown, exceptPeerId?: string): void {
        for (const peer of this.peers.values()) {
            if (peer.id !== exceptPeerId) {
                peer.send(event, data);
            }
        }
    }

    public isEmpty(): boolean {
        return this.peers.size === 0;
    }

    public close(): void {
        for (const timer of this.evictions.values()) {
            clearTimeout(timer);
        }

        this.evictions.clear();
        this.router.close();
        this.peers.clear();
    }
}
