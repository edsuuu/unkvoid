import { ProducerResource } from '../Resources/ProducerResource.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ProducerController {
    async store(request) {
        const peer = request.peer();
        const room = request.room();

        const producer = await peer.getTransport(request.transportId()).produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
        });

        peer.addProducer(producer, request.source());
        producer.on('transportclose', () => peer.producers.delete(producer.id));

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

    destroy(request) {
        const peer = request.peer();
        const producer = peer.producers.get(request.producerId());

        if (producer) {
            producer.close();
            peer.producers.delete(producer.id);
            request.room().broadcast('producerClosed', { peerId: peer.id, producerId: producer.id }, peer.id);
        }

        return new StatusResource('closed');
    }
}
