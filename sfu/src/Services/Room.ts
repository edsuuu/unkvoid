import type { Producer, Router, WebRtcServer, WebRtcTransport, Worker } from 'mediasoup/types';
import type { WebSocket } from 'ws';

import { config } from '../config.js';
import { NotFoundException } from '../Exceptions/ApiException.js';
import type { PeerDescription } from '../types.js';
import { Peer } from './Peer.js';

type ProducerOwner = { peer: Peer; producer: Producer };

export type JoinOutcome = { peer: Peer; resumed: boolean };

/**
 * Quanto tempo a sessão sobrevive sem sinalização. A mídia WebRTC não cai junto com
 * o WebSocket, então segurar o participante aqui faz uma queda de rede virar um
 * soluço em vez de queda de chamada.
 */
const GRACE_MS = 45_000;

export class Room {
    public readonly peers = new Map<string, Peer>();

    /** Avisado quando a carência de um órfão estoura, para a sala poder ser liberada. */
    public onEvicted: ((room: Room) => void) | null = null;

    /** Avisado quando alguém sai de verdade, para a presença do servidor atualizar. */
    public onPeerGone: ((roomId: string, peerId: string) => void) | null = null;

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
     * Três caminhos: retomar uma sessão órfã (mídia intacta), derrubar uma sessão
     * viva de outra aba, ou criar do zero.
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

            return { peer: previous, resumed: true };
        }

        if (previous) {
            this.cancelEviction(id);
            previous.send('replaced', { reason: 'você entrou neste canal em outra aba' });
            this.peers.delete(id);
            previous.close();
            previous.socket.close();
        }

        const peer = new Peer(id, name, socket, options);

        this.peers.set(peer.id, peer);

        return { peer, resumed: false };
    }

    /**
     * Sinalização caiu: segura o participante por GRACE_MS antes de destruir. Só
     * avisa a sala quando a carência estoura de verdade.
     */
    orphanPeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.orphanedAt = Date.now();

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

    /** Participantes com sinalização viva. Órfãos não contam para fechar a sala. */
    activeCount(): number {
        return [...this.peers.values()].filter(peer => ! peer.isOrphaned()).length;
    }

    findPeer(peerId: string): Peer {
        const peer = this.peers.get(peerId);

        if (!peer) {
            throw new NotFoundException(`participante ${peerId} não está nesta sala`);
        }

        return peer;
    }

    /**
     * Recebe o objeto, não o id: fechar o socket de uma sessão substituída não pode
     * derrubar a sessão nova, que carrega o mesmo id de participante.
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

        throw new NotFoundException(`producer ${producerId} não existe nesta sala`);
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
