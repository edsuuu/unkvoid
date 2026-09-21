import type { Consumer } from 'mediasoup/types';

import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { Peer } from '../../Services/Peer.js';
import type { ProducerOwner, Room } from '../../Services/Room.js';
import type { ConsumePlainRequest } from '../Request/ConsumePlainRequest.js';
import type { ConsumeRequest } from '../Request/ConsumeRequest.js';
import type { ConsumerRequest } from '../Request/ConsumerRequest.js';

export class ConsumerController {
    public async store(request: ConsumeRequest): Promise<Payload> {
        const room = request.room();
        const peer = request.peer();

        if (
            !room.router.canConsume({
                producerId: request.producerId(),
                rtpCapabilities: request.rtpCapabilities(),
            })
        ) {
            throw new ValidationException('this participant cannot receive this media');
        }

        const owner = room.findProducerOwner(request.producerId());

        const consumer = await peer.getTransport(request.transportId()).consume({
            producerId: request.producerId(),
            rtpCapabilities: request.rtpCapabilities(),
            paused: true,
        });

        peer.consumers.set(consumer.id, consumer);

        this.track(room, peer, consumer, owner);

        return {
            consumerId: consumer.id,
            producerId: consumer.producerId,
            kind: consumer.kind,
            rtpParameters: consumer.rtpParameters,
            peerId: owner.peer.id,
            name: owner.peer.name,
            source: String(owner.producer.appData.source),
        };
    }

    public async storePlain(request: ConsumePlainRequest): Promise<Payload> {
        const room = request.room();
        const peer = request.peer();
        const owner = room.findProducerOwner(request.producerId());
        const transport = await room.plainReceiveTransportFor(peer, request.srtpParameters());

        const consumer = await transport.consume({
            producerId: request.producerId(),
            rtpCapabilities: room.router.rtpCapabilities,
            paused: true,
        });

        peer.consumers.set(consumer.id, consumer);

        this.track(room, peer, consumer, owner);

        const codec = consumer.rtpParameters.codecs[0];

        return {
            consumerId: consumer.id,
            producerId: consumer.producerId,
            kind: consumer.kind,
            payloadType: codec?.payloadType ?? null,
            clockRate: codec?.clockRate ?? null,
            ssrc: consumer.rtpParameters.encodings?.[0]?.ssrc ?? null,
            ip: transport.tuple.localAddress,
            port: transport.tuple.localPort,
            srtpParameters: transport.srtpParameters,
            peerId: owner.peer.id,
            name: owner.peer.name,
            source: String(owner.producer.appData.source),
        };
    }

    private track(room: Room, peer: Peer, consumer: Consumer, owner: ProducerOwner): void {
        consumer.on('transportclose', () => {
            peer.consumers.delete(consumer.id);
            room.announceWatchers(consumer.producerId);
        });

        consumer.on('producerclose', () => {
            peer.consumers.delete(consumer.id);
            peer.send('consumerClosed', {
                consumerId: consumer.id,
                producerId: consumer.producerId,
                kind: consumer.kind,

                peerId: owner.peer.id,
                source: String(owner.producer.appData.source),
            });
        });
    }

    public async resume(request: ConsumerRequest): Promise<Payload> {
        const consumer = request.peer().getConsumer(request.consumerId());
        await consumer.resume();

        if (consumer.kind === 'video') {
            await consumer.requestKeyFrame();
        }

        request.room().announceWatchers(consumer.producerId);

        return { status: 'resumed' };
    }

    public async pause(request: ConsumerRequest): Promise<Payload> {
        const consumer = request.peer().getConsumer(request.consumerId());

        await consumer.pause();
        request.room().announceWatchers(consumer.producerId);

        return { status: 'paused' };
    }

    public destroy(request: ConsumerRequest): Payload {
        const peer = request.peer();
        const consumer = peer.getConsumer(request.consumerId());
        const { producerId, kind } = consumer;

        consumer.close();
        peer.consumers.delete(request.consumerId());

        if (kind === 'video') {
            request.room().announceWatchers(producerId);
        }

        return { status: 'closed' };
    }
}
