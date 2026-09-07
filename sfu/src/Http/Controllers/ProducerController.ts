import type { Producer } from 'mediasoup/types';

import { Source, type SourceName } from '../../Enums/Source.js';
import type { Peer } from '../../Services/Peer.js';
import type { PresenceRegistry } from '../../Services/PresenceRegistry.js';
import type { Room } from '../../Services/Room.js';
import type { ProduceRequest } from '../Requests/ProduceRequest.js';
import type { ProducePlainRequest } from '../Requests/ProducePlainRequest.js';
import type { ProducerRequest } from '../Requests/ProducerRequest.js';
import { PlainProducerResource } from '../Resources/PlainProducerResource.js';
import { ProducerResource } from '../Resources/ProducerResource.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ProducerController {
    constructor(private readonly presence: PresenceRegistry) {}

    async store(request: ProduceRequest): Promise<ProducerResource> {
        const peer = request.peer();
        const room = request.room();

        const producer = await peer.getTransport(request.transportId()).produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
        });

        this.announce(peer, room, producer, request.source());

        return new ProducerResource(producer);
    }

    /**
     * Same broadcast, arriving as plain RTP instead of through WebRTC. This is how the
     * native app reaches more viewers than direct connections can carry: it keeps
     * encoding once on the GPU, but uploads once to the server instead of once per
     * viewer, and the server fans it out.
     */
    async storePlain(request: ProducePlainRequest): Promise<PlainProducerResource> {
        const peer = request.peer();
        const room = request.room();
        const transport = await room.createPlainTransport(peer, request.srtpParameters());

        const producer = await transport.produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
        });

        this.announce(peer, room, producer, request.source());

        return new PlainProducerResource(producer, transport);
    }

    /** Registers the producer and tells the room, whichever transport it arrived on. */
    private announce(peer: Peer, room: Room, producer: Producer, source: SourceName): void {
        peer.addProducer(producer, source);
        producer.on('transportclose', () => peer.producers.delete(producer.id));

        // A plain producer is declared before a single packet arrives, so until the score
        // rises the broadcaster has no way to tell "the server is receiving" from "my
        // packets are going nowhere". Reported once: after that the score only fluctuates.
        let receiving = false;

        producer.on('score', scores => {
            if (receiving || ! scores.some(entry => entry.score > 0)) {
                return;
            }

            receiving = true;
            peer.send('producerActive', { producerId: producer.id });
        });

        if (source === Source.Screen) {
            this.presence.setSharing(room.id, peer.id, true, producer.id);
        }

        room.broadcast('newProducer', {
            peerId: peer.id,
            name: peer.name,
            avatar: peer.avatar,
            producerId: producer.id,
            kind: producer.kind,
            source,
        }, peer.id);
    }

    destroy(request: ProducerRequest): StatusResource {
        const peer = request.peer();
        const producer = peer.producers.get(request.producerId());

        if (producer) {
            const source = String(producer.appData.source);

            producer.close();
            peer.producers.delete(producer.id);
            request.room().broadcast('producerClosed', { peerId: peer.id, producerId: producer.id }, peer.id);

            if (source === Source.Screen) {
                this.presence.setSharing(request.room().id, peer.id, false);
            }
        }

        return new StatusResource('closed');
    }
}
