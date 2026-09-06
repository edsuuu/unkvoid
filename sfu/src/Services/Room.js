import { config } from '../config.js';
import { NotFoundException } from '../Exceptions/ApiException.js';
import { Peer } from './Peer.js';

export class Room {
    constructor(id, router, webRtcServer) {
        this.id = id;
        this.router = router;
        this.webRtcServer = webRtcServer;
        this.peers = new Map();
    }

    static async create(worker, webRtcServer, id) {
        const router = await worker.createRouter({ mediaCodecs: config.router.mediaCodecs });

        return new Room(id, router, webRtcServer);
    }

    /**
     * A sessão nova sempre vence: entrar de outra aba derruba a anterior, em vez de
     * ser recusado. Recusar deixava a pessoa presa se um socket morresse sem fechar.
     */
    addPeer(id, name, socket, options) {
        const previous = this.peers.get(id);

        if (previous) {
            previous.send('replaced', { reason: 'você entrou neste canal em outra aba' });
            this.peers.delete(id);
            previous.close();
            previous.socket.close();
        }

        const peer = new Peer(id, name, socket, options);

        this.peers.set(peer.id, peer);

        return peer;
    }

    findPeer(peerId) {
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
    removePeer(peer) {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.close();
        this.peers.delete(peer.id);
        this.broadcast('peerLeft', { peerId: peer.id }, peer.id);
    }

    describePeers(exceptPeerId) {
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

    async createTransport(peer) {
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

    findProducerOwner(producerId) {
        for (const peer of this.peers.values()) {
            const producer = peer.producers.get(producerId);

            if (producer) {
                return { peer, producer };
            }
        }

        throw new NotFoundException(`producer ${producerId} não existe nesta sala`);
    }

    broadcast(event, data, exceptPeerId) {
        for (const peer of this.peers.values()) {
            if (peer.id !== exceptPeerId) {
                peer.send(event, data);
            }
        }
    }

    isEmpty() {
        return this.peers.size === 0;
    }

    close() {
        this.router.close();
        this.peers.clear();
    }
}
