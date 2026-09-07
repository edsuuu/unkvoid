import { ValidationException } from '../../Exceptions/ApiException.js';
import type { ConsumeRequest } from '../Requests/ConsumeRequest.js';
import type { ConsumerRequest } from '../Requests/ConsumerRequest.js';
import { ConsumerResource } from '../Resources/ConsumerResource.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ConsumerController {
    async store(request: ConsumeRequest): Promise<ConsumerResource> {
        const room = request.room();
        const peer = request.peer();

        if (! room.router.canConsume({ producerId: request.producerId(), rtpCapabilities: request.rtpCapabilities() })) {
            throw new ValidationException('this participant cannot receive this media');
        }

        const owner = room.findProducerOwner(request.producerId());

        // It starts paused intentionally: resume only after the client confirms the
        // the consumer avoids losing the initial keyframe and showing a black screen.
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

    async resume(request: ConsumerRequest): Promise<StatusResource> {
        await request.peer().getConsumer(request.consumerId()).resume();

        return new StatusResource('resumed');
    }

    async pause(request: ConsumerRequest): Promise<StatusResource> {
        await request.peer().getConsumer(request.consumerId()).pause();

        return new StatusResource('paused');
    }

    async setPreferredLayers(request: ConsumerRequest): Promise<StatusResource> {
        await request.peer().getConsumer(request.consumerId()).setPreferredLayers({
            spatialLayer: request.spatialLayer(),
            temporalLayer: request.temporalLayer(),
        });

        return new StatusResource('layers-set');
    }
}
