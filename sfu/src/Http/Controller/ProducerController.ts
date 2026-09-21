import type { Producer } from 'mediasoup/types';

import type { SourceName } from '../../Enums/Source.js';
import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import type { ProducePlainRequest } from '../Request/ProducePlainRequest.js';
import type { ProduceRequest } from '../Request/ProduceRequest.js';
import type { ProducerRequest } from '../Request/ProducerRequest.js';

const MEDIA_IDLE_MS = 30_000;

export class ProducerController {
    public async store(request: ProduceRequest): Promise<Payload> {
        const peer = request.peer();
        const room = request.room();

        peer.assertCanProduce(request.source());

        const producer = await peer.getTransport(request.transportId()).produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
            appData: { source: request.source() },
        });

        this.announce(peer, room, producer, request.source());

        return {
            producerId: producer.id,
            kind: producer.kind,
            source: String(producer.appData.source),
        };
    }

    public async storePlain(request: ProducePlainRequest): Promise<Payload> {
        const peer = request.peer();
        const room = request.room();

        peer.assertCanProduce(request.source());

        const transport = await room.plainTransportFor(peer, request.srtpParameters());

        const producer = await transport.produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
            appData: { plain: true },
        });

        this.announce(peer, room, producer, request.source());

        return {
            producerId: producer.id,
            kind: producer.kind,
            source: request.source(),
            ip: transport.tuple.localAddress,
            port: transport.tuple.localPort,
            srtpParameters: transport.srtpParameters,
        };
    }

    private announce(peer: Peer, room: Room, producer: Producer, source: SourceName): void {
        peer.addProducer(producer, source);

        let idleTimer: ReturnType<typeof setTimeout> | undefined = setTimeout(() => {
            console.warn(
                `[WARN] producerDead room=${room.id} sub=${peer.userId} source=${source} ip=${peer.ip}: no packet in ${MEDIA_IDLE_MS / 1000}s`,
            );
            peer.send('producerDead', { producerId: producer.id, kind: producer.kind, source });
            room.closeProducer(peer, producer);
        }, MEDIA_IDLE_MS);

        producer.observer.once('close', () => clearTimeout(idleTimer));

        producer.on('transportclose', () => {
            peer.producers.delete(producer.id);
            room.broadcast(
                'producerClosed',
                {
                    peerId: peer.id,
                    producerId: producer.id,
                    kind: producer.kind,
                    source,
                },
                peer.id,
            );
        });

        let receiving = false;

        producer.on('score', (scores) => {
            if (receiving || !scores.some((entry) => entry.score > 0)) {
                return;
            }

            receiving = true;
            if (idleTimer) {
                clearTimeout(idleTimer);
                idleTimer = undefined;
            }
            peer.send('producerActive', { producerId: producer.id });
        });

        room.broadcast(
            'newProducer',
            {
                peerId: peer.id,
                name: peer.name,
                producerId: producer.id,
                kind: producer.kind,
                source,
            },
            peer.id,
        );
    }

    public async pause(request: ProducerRequest): Promise<Payload> {
        const peer = request.peer();

        await request.room().setProducerPaused(peer, peer.getProducer(request.producerId()), true);

        return { status: 'paused' };
    }

    public async resume(request: ProducerRequest): Promise<Payload> {
        const peer = request.peer();

        await request.room().setProducerPaused(peer, peer.getProducer(request.producerId()), false);

        return { status: 'resumed' };
    }

    public destroy(request: ProducerRequest): Payload {
        const peer = request.peer();

        request.room().closeProducer(peer, peer.producers.get(request.producerId()));

        return { status: 'closed' };
    }
}
