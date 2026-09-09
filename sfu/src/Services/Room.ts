import { randomUUID, randomBytes } from 'node:crypto';

import type { PlainTransport, Producer, Router, SrtpParameters, WebRtcServer, WebRtcTransport, Worker } from 'mediasoup/types';
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

    /** Devolve a sala ao registro quando ela esvazia. O registro é quem ignora se não esvaziou. */
    public onEvicted: ((room: Room) => void) | null = null;

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
     * Três caminhos: retomar uma sessão órfã (mídia intacta), encerrar a sessão anterior
     * da mesma pessoa, ou criar uma do zero.
     *
     * Quem prova ser a mesma pessoa é a `resumeKey`, e não o `peerId`: o id a sala
     * inteira recebe no `peerJoined`, então aceitá-lo como identidade deixaria qualquer
     * um derrubar qualquer um só entrando com o id alheio.
     */
    addPeer(
        name: string,
        socket: WebSocket,
        options: { resumeKey?: string | null; resume?: boolean } = {},
    ): JoinOutcome {
        const previous = options.resumeKey ? this.findByResumeKey(options.resumeKey) : null;

        if (previous?.isOrphaned() && options.resume) {
            this.cancelEviction(previous.id);
            previous.attachSocket(socket);
            this.broadcast('peerReconnected', { peerId: previous.id }, previous.id);

            return { peer: previous, resumed: true };
        }

        if (previous) {
            this.cancelEviction(previous.id);
            previous.send('replaced', { reason: 'you opened this room in another window' });
            this.peers.delete(previous.id);
            previous.close();
            previous.socket.close();
        }

        const peer = new Peer(randomUUID(), name, socket, randomBytes(16).toString('hex'));

        this.peers.set(peer.id, peer);

        return { peer, resumed: false };
    }

    private findByResumeKey(resumeKey: string): Peer | null {
        // Varredura porque sala é coisa de dezenas, não de milhares: um índice a mais
        // seria outra estrutura para manter em sincronia com esta.
        return [...this.peers.values()].find(peer => peer.resumeKey === resumeKey) ?? null;
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

        this.evictions.set(peer.id, setTimeout(() => {
            this.evictions.delete(peer.id);

            if (this.peers.get(peer.id) === peer && peer.isOrphaned()) {
                this.removePeer(peer);
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

        // Sair de propósito também esvazia a sala. Sem isto, só a expiração da carência
        // devolvia o router ao registro, e uma sala de onde todo mundo saiu no botão
        // ficava alocada até o processo reiniciar.
        this.onEvicted?.(this);
    }

    describePeers(exceptPeerId?: string): PeerDescription[] {
        return [...this.peers.values()]
            .filter(peer => peer.id !== exceptPeerId && ! peer.isOrphaned())
            .map(peer => ({
                peerId: peer.id,
                name: peer.name,
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

    /**
     * Ingest de quem não é navegador: o app já codificou H.264 na GPU e manda RTP direto
     * nesta porta, sem ICE e sem DTLS.
     *
     * `comedia` faz o transport aprender o endereço do remetente no primeiro pacote, então
     * o app não precisa de porta alcançável — que é o ponto, já que ele vive atrás do
     * roteador de casa. SRTP não é opcional: sem ele a tela atravessaria a internet limpa.
     *
     * Uma transmissão, um transport: vídeo e áudio dividem. O mediasoup aceita vários
     * `produce()` no mesmo transport e o SSRC já distingue um do outro — um transport por
     * mídia gastaria o dobro de portas UDP, e cada porta a mais é uma linha a mais na
     * regra de firewall que alguém cria à mão.
     */
    async plainTransportFor(peer: Peer, srtpParameters: SrtpParameters): Promise<PlainTransport> {
        const existing = [...peer.plainTransports.values()].at(0);

        if (existing) {
            return existing;
        }

        const transport = await this.router.createPlainTransport({
            listenInfo: { protocol: 'udp', ip: '0.0.0.0', announcedAddress: config.announcedAddress },
            rtcpMux: true,
            comedia: true,
            enableSrtp: true,
            srtpCryptoSuite: srtpParameters.cryptoSuite,
        });

        await transport.connect({ srtpParameters });

        peer.addPlainTransport(transport);

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
