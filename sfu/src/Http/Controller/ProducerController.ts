import type { PlainTransport, Producer } from 'mediasoup/types';

import type { SourceName } from '../../Enums/Source.js';
import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import type { ProducePlainRequest } from '../Request/ProducePlainRequest.js';
import type { ProduceRequest } from '../Request/ProduceRequest.js';
import type { ProducerRequest } from '../Request/ProducerRequest.js';

const MEDIA_IDLE_MS = 30_000;

const KEYFRAME_REQUEST_DELAY_MS = 500;

const ARRIVAL_POLL_MS = 1000;

const ARRIVAL_SILENT_POLLS = 2;

export class ProducerController {
    public async store(request: ProduceRequest): Promise<Payload> {
        const peer = request.peer();
        const room = request.room();

        peer.assertCanProduce(request.source());

        const transport = peer.getTransport(request.transportId());
        const producer = await transport.produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
            keyFrameRequestDelay: KEYFRAME_REQUEST_DELAY_MS,
            appData: { source: request.source() },
        });

        peer.assertStillOpen(transport, producer);
        await this.announce(peer, room, producer, request.source());

        return {
            producerId: producer.id,
            kind: producer.kind,
            source: String(producer.appData.source),
            paused: producer.paused,
        };
    }

    public async storePlain(request: ProducePlainRequest): Promise<Payload> {
        const peer = request.peer();
        const room = request.room();

        peer.assertCanProduce(request.source());

        peer.producingPlain += 1;

        let transport: PlainTransport;
        let producer: Producer;

        try {
            transport = await room.plainTransportFor(peer, request.srtpParameters());
            producer = await transport.produce({
                kind: request.kind(),
                rtpParameters: request.rtpParameters(),
                keyFrameRequestDelay: KEYFRAME_REQUEST_DELAY_MS,
                appData: { plain: true },
            });
        } finally {
            peer.producingPlain -= 1;
        }

        peer.assertStillOpen(transport, producer);
        await this.announce(peer, room, producer, request.source());

        return {
            producerId: producer.id,
            kind: producer.kind,
            source: request.source(),
            paused: producer.paused,
            ip: transport.tuple.localAddress,
            port: transport.tuple.localPort,
            srtpParameters: transport.srtpParameters,
        };
    }

    private async announce(
        peer: Peer,
        room: Room,
        producer: Producer,
        source: SourceName,
    ): Promise<void> {
        // A retomada com `can` menor pode ter chegado enquanto o worker criava o producer: a
        // permissão conferida antes dos `await` já não vale.
        if (!peer.allows(source)) {
            producer.close();
            peer.assertCanProduce(source);
        }

        peer.addProducer(producer, source);

        // Mutado pelo servidor: o mic sobe, mas calado, e o `/mute false` o retoma.
        if (peer.serverMuted && source === 'mic') {
            await producer.pause();
        }

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

        this.watchArrival(producer, (receiving) => {
            room.broadcast('producerReceiving', { producerId: producer.id, receiving });

            if (receiving && idleTimer) {
                clearTimeout(idleTimer);
                idleTimer = undefined;
                peer.send('producerActive', { producerId: producer.id });
            }
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

    /**
     * Se o RTP do producer está chegando aqui, contado pelo próprio SFU: a nota do mediasoup
     * só zera por inatividade em simulcast, e o app manda um fluxo só — quem transmite parava
     * e a sala seguia ouvindo `receiving: true` para sempre. Para depois de dois intervalos
     * sem pacote (2 a 3 s) e na hora em que o producer pausa, porque pausado nada sai daqui. É
     * assim que quem assiste separa a tela parada de quem transmite (nada chega aqui) do
     * caminho até ele que morreu (chega aqui e não lá).
     */
    private watchArrival(producer: Producer, changed: (receiving: boolean) => void): void {
        let counted = 0;
        let silent = 0;
        let polling = false;

        const timer = setInterval(() => {
            if (polling) {
                return;
            }

            polling = true;
            producer
                .getStats()
                .then((stats) => {
                    const packets = stats.reduce((total, entry) => total + entry.packetCount, 0);

                    silent = packets > counted ? 0 : silent + 1;
                    counted = packets;

                    const was = producer.appData.receiving === true;
                    const now =
                        !producer.closed &&
                        !producer.paused &&
                        (silent === 0 || (was && silent < ARRIVAL_SILENT_POLLS));

                    if (now !== was) {
                        producer.appData.receiving = now;
                        changed(now);
                    }
                })
                .catch(() => undefined)
                .finally(() => (polling = false));
        }, ARRIVAL_POLL_MS);

        producer.observer.once('close', () => clearInterval(timer));
    }

    public async pause(request: ProducerRequest): Promise<Payload> {
        const peer = request.peer();

        await request
            .room()
            .setOwnProducerPaused(peer, peer.getProducer(request.producerId()), true);

        return { status: 'paused' };
    }

    public async resume(request: ProducerRequest): Promise<Payload> {
        const peer = request.peer();

        await request
            .room()
            .setOwnProducerPaused(peer, peer.getProducer(request.producerId()), false);

        return { status: 'resumed' };
    }

    public destroy(request: ProducerRequest): Payload {
        const peer = request.peer();

        request.room().closeProducer(peer, peer.producers.get(request.producerId()));

        return { status: 'closed' };
    }
}
