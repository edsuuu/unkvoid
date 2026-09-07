import { Source } from '../../Enums/Source.js';
import type { PresenceRegistry } from '../../Services/PresenceRegistry.js';
import type { ProduceRequest } from '../Requests/ProduceRequest.js';
import type { ProducerRequest } from '../Requests/ProducerRequest.js';
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

        peer.addProducer(producer, request.source());
        producer.on('transportclose', () => peer.producers.delete(producer.id));

        if (request.source() === Source.Screen) {
            this.presence.setSharing(room.id, peer.id, true, producer.id);
        }

        room.broadcast('newProducer', {
            peerId: peer.id,
            name: peer.name,
            avatar: peer.avatar,
            producerId: producer.id,
            kind: producer.kind,
            source: request.source(),
        }, peer.id);

        return new ProducerResource(producer);
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
