import { ValidationException } from '../../Exceptions/ApiException.js';
import { ConsumerResource } from '../Resources/ConsumerResource.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ConsumerController {
    async store(request) {
        const room = request.room();
        const peer = request.peer();

        if (! room.router.canConsume({ producerId: request.producerId(), rtpCapabilities: request.rtpCapabilities() })) {
            throw new ValidationException('este participante não consegue receber esta mídia');
        }

        const owner = room.findProducerOwner(request.producerId());

        // Nasce pausado de propósito: retomar só depois que o cliente confirma o
        // consumer evita perder o keyframe inicial e a tela abrir preta.
        const consumer = await peer.getTransport(request.transportId()).consume({
            producerId: request.producerId(),
            rtpCapabilities: request.rtpCapabilities(),
            paused: true,
        });

        peer.consumers.set(consumer.id, consumer);
        consumer.on('transportclose', () => peer.consumers.delete(consumer.id));
        consumer.on('producerclose', () => {
            peer.consumers.delete(consumer.id);
            peer.send('consumerClosed', { consumerId: consumer.id });
        });

        return new ConsumerResource(consumer, owner);
    }

    async resume(request) {
        await request.peer().getConsumer(request.consumerId()).resume();

        return new StatusResource('resumed');
    }

    async setPreferredLayers(request) {
        await request.peer().getConsumer(request.consumerId()).setPreferredLayers({
            spatialLayer: request.spatialLayer(),
            temporalLayer: request.temporalLayer(),
        });

        return new StatusResource('layers-set');
    }
}
