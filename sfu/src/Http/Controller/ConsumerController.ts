import type { Consumer, RtpCapabilities, Transport } from 'mediasoup/types';

import { NotFoundException, ValidationException } from '../../Exceptions/ApiException.js';
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
        const transport = peer.getTransport(request.transportId());
        const owner = room.findProducerOwner(request.producerId());
        const media = await room.routerOf(peer);

        await room.pipe(request.producerId(), media);

        if (
            !media.router.canConsume({
                producerId: request.producerId(),
                rtpCapabilities: request.rtpCapabilities(),
            })
        ) {
            throw new ValidationException('this participant cannot receive this media');
        }

        const consumer = await transport.consume({
            producerId: request.producerId(),
            rtpCapabilities: request.rtpCapabilities(),
            paused: true,
        });

        this.assertStillWanted(peer, transport, consumer, owner);
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
        const media = await room.routerOf(peer);

        await room.pipe(request.producerId(), media);

        if (owner.producer.closed) {
            throw new NotFoundException(`producer ${request.producerId()} already closed`);
        }

        const consumer = await transport.consume({
            producerId: request.producerId(),
            rtpCapabilities: ConsumerController.plainCapabilities(media.router.rtpCapabilities),
            paused: true,
        });

        this.assertStillWanted(peer, transport, consumer, owner);
        peer.consumers.set(consumer.id, consumer);

        this.track(room, peer, consumer, owner);

        const codec = consumer.rtpParameters.codecs[0];
        const repair = consumer.rtpParameters.codecs.find(
            (candidate) =>
                candidate.mimeType.toLowerCase().endsWith('/rtx') &&
                candidate.parameters?.apt === codec?.payloadType,
        );
        const repairSsrc = consumer.rtpParameters.encodings?.[0]?.rtx?.ssrc;

        return {
            consumerId: consumer.id,
            producerId: consumer.producerId,
            kind: consumer.kind,
            payloadType: codec?.payloadType ?? null,
            clockRate: codec?.clockRate ?? null,
            ssrc: consumer.rtpParameters.encodings?.[0]?.ssrc ?? null,
            rtx:
                repair && repairSsrc ? { ssrc: repairSsrc, payloadType: repair.payloadType } : null,
            ip: transport.tuple.localAddress,
            port: transport.tuple.localPort,
            srtpParameters: transport.srtpParameters,
            peerId: owner.peer.id,
            name: owner.peer.name,
            source: String(owner.producer.appData.source),
            receiving: owner.producer.appData.receiving === true && !owner.producer.paused,
        };
    }

    /**
     * O que o receptor nativo usa: o codec, o NACK, o PLI e o RTX. O transport-cc e o REMB do
     * router são do navegador: negociados no RTP puro, o mediasoup sondava a banda de quem
     * assiste com pacotes de sondagem (o SSRC 1234) que o app nunca usa nem responde. O bloco de
     * extensões que o mediasoup reescreve em todo pacote fica, negociado ou não: o receptor o pula.
     */
    private static plainCapabilities(router: RtpCapabilities): RtpCapabilities {
        return {
            codecs: (router.codecs ?? []).map((codec) => ({
                ...codec,
                rtcpFeedback: (codec.rtcpFeedback ?? []).filter(
                    (feedback) => feedback.type !== 'transport-cc' && feedback.type !== 'goog-remb',
                ),
            })),
            headerExtensions: [],
        };
    }

    /** O transporte pode ter fechado, ou a tela acabado, enquanto o worker criava o consumer. */
    private assertStillWanted(
        peer: Peer,
        transport: Transport,
        consumer: Consumer,
        owner: ProducerOwner,
    ): void {
        peer.assertStillOpen(transport, consumer);

        if (owner.producer.closed) {
            consumer.close();
            throw new NotFoundException(`producer ${owner.producer.id} already closed`);
        }
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

        // O `resume` já pede o quadro-chave (e o mediasoup pede de novo quando o caminho do
        // consumer se conecta). Um segundo pedido aqui caía no freio de 1 s do
        // `keyFrameRequestDelay`, saía um segundo depois e rearmava o freio: cada pessoa que
        // abria uma tela custava dois quadros-chave a quem transmite, e quem entrava logo
        // depois esperava até um segundo a mais pela primeira imagem.
        await consumer.resume();

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
