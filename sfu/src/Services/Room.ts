import type { Producer, Router, WebRtcServer, WebRtcTransport, Worker } from 'mediasoup/types';
import type { WebSocket } from 'ws';

import { config } from '../config.js';
import { NotFoundException } from '../Exceptions/ApiException.js';
import type { PeerDescription } from '../types.js';
import { Peer } from './Peer.js';

type ProducerOwner = { peer: Peer; producer: Producer };

export type JoinOutcome = { peer: Peer; resumed: boolean };

/**
 * How long the session survives without signaling. WebRTC media does not drop with
 * the WebSocket, so keeping the participant here turns a network drop into a
 * hiccup instead of a call drop.
 */
const GRACE_MS = 45_000;

export class Room {
    public readonly peers = new Map<string, Peer>();

    /** Notified when an orphan’s grace period expires so the room can be released. */
    public onEvicted: ((room: Room) => void) | null = null;

    /** Notified when someone truly leaves so server presence can update. */
    public onPeerGone: ((roomId: string, peerId: string) => void) | null = null;

    /** Notified when someone’s signaling drops, before the grace period expires. */
    public onPeerOrphaned: ((roomId: string, peerId: string) => void) | null = null;

    private readonly evictions = new Map<string, NodeJS.Timeout>();

    constructor(
        public readonly id: string,
        public readonly router: Router,
        private readonly webRtcServer: WebRtcServer,
    ) {}

    static async create(worker: Worker, webRtcServer: WebRtcServer, id: string): Promise<Room> {
        const router = await worker.createRouter({ mediaCodecs: config.router.mediaCodecs });

        return new Room(id, router, webRtcServer);
    }

    /**
     * Three paths: resume an orphaned session (media intact), terminate a session
     * from another tab, or create one from scratch.
     */
    addPeer(
        id: string,
        name: string,
        socket: WebSocket,
        options: { role?: Peer['role']; avatar?: string | null; resume?: boolean },
    ): JoinOutcome {
        const previous = this.peers.get(id);

        if (previous?.isOrphaned() && options.resume) {
            this.cancelEviction(id);
            previous.attachSocket(socket);
            this.broadcast('peerReconnected', { peerId: id }, id);

            return { peer: previous, resumed: true };
        }

        if (previous) {
            this.cancelEviction(id);
            previous.send('replaced', { reason: 'you joined this channel in another tab' });
            this.peers.delete(id);
            previous.close();
            previous.socket.close();
        }

        const peer = new Peer(id, name, socket, options);

        this.peers.set(peer.id, peer);

        return { peer, resumed: false };
    }

    /**
     * Signaling dropped: hold the participant for GRACE_MS before destroying it. Only
     * notify the room when the grace period truly expires.
     */
    orphanPeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.orphanedAt = Date.now();

        // Notify the room immediately: without this, viewers were left with the last frame
        // frozen, unaware that the broadcaster’s connection dropped.
        this.broadcast('peerConnectionLost', { peerId: peer.id }, peer.id);
        this.onPeerOrphaned?.(this.id, peer.id);

        this.evictions.set(peer.id, setTimeout(() => {
            this.evictions.delete(peer.id);

            if (this.peers.get(peer.id) === peer && peer.isOrphaned()) {
                this.removePeer(peer);
                this.onEvicted?.(this);
            }
        }, GRACE_MS));
    }

    private cancelEviction(peerId: string): void {
        const timer = this.evictions.get(peerId);

        if (timer) {
            clearTimeout(timer);
            this.evictions.delete(peerId);
        }
    }

    /** Participants with live signaling. Orphans do not count when closing the room. */
    activeCount(): number {
        return [...this.peers.values()].filter(peer => ! peer.isOrphaned()).length;
    }

    findPeer(peerId: string): Peer {
        const peer = this.peers.get(peerId);

        if (!peer) {
            throw new NotFoundException(`participant ${peerId} is not in this room`);
        }

        return peer;
    }

    /**
     * Takes the object, not the ID: closing the socket of a replaced session must not
     * terminate the new session, which carries the same participant ID.
     */
    removePeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.close();
        this.peers.delete(peer.id);
        this.broadcast('peerLeft', { peerId: peer.id }, peer.id);
        this.onPeerGone?.(this.id, peer.id);
    }

    describePeers(exceptPeerId?: string): PeerDescription[] {
        return [...this.peers.values()]
            .filter(peer => peer.id !== exceptPeerId)
            .map(peer => ({
                peerId: peer.id,
                name: peer.name,
                avatar: peer.avatar,
                role: peer.role,
                producers: peer.describeProducers(),
            }));
    }

    async createTransport(peer: Peer): Promise<WebRtcTransport> {
        const transport = await this.router.createWebRtcTransport({
            webRtcServer: this.webRtcServer,
            enableUdp: config.transport.enableUdp,
            enableTcp: config.transport.enableTcp,
            preferUdp: config.transport.preferUdp,
            initialAvailableOutgoingBitrate: config.transport.initialAvailableOutgoingBitrate,
        });

        await transport.setMaxIncomingBitrate(config.transport.maxIncomingBitrate);

        transport.on('dtlsstatechange', state => {
            if (state === 'closed') {
                transport.close();
            }
        });

        peer.addTransport(transport);

        return transport;
    }

    findProducerOwner(producerId: string): ProducerOwner {
        for (const peer of this.peers.values()) {
            const producer = peer.producers.get(producerId);

            if (producer) {
                return { peer, producer };
            }
        }

        throw new NotFoundException(`producer ${producerId} does not exist in this room`);
    }

    broadcast(event: string, data: unknown, exceptPeerId?: string): void {
        for (const peer of this.peers.values()) {
            if (peer.id !== exceptPeerId) {
                peer.send(event, data);
            }
        }
    }

    isEmpty(): boolean {
        return this.peers.size === 0;
    }

    close(): void {
        for (const timer of this.evictions.values()) {
            clearTimeout(timer);
        }

        this.evictions.clear();
        this.router.close();
        this.peers.clear();
    }
}
