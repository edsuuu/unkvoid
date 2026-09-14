import type { Producer } from 'mediasoup/types';

import type { SourceName } from '../../Enums/Source.js';
import type { Peer } from '../../Services/Peer.js';
import { Recorder } from '../../Services/Recorder.js';
import type { Room } from '../../Services/Room.js';
import type { ProducePlainRequest } from '../Requests/ProducePlainRequest.js';
import type { ProduceRequest } from '../Requests/ProduceRequest.js';
import type { ProducerRequest } from '../Requests/ProducerRequest.js';
import { PlainProducerResource } from '../Resources/PlainProducerResource.js';
import { ProducerResource } from '../Resources/ProducerResource.js';
import { StatusResource } from '../Resources/StatusResource.js';

const MEDIA_IDLE_MS = 30_000;

export class ProducerController {
    /** Mic, câmera e tela de quem tem WebRTC na janela. */
    public async store(request: ProduceRequest): Promise<ProducerResource> {
        const peer = request.peer();
        const room = request.room();

        peer.assertCanProduce(request.source());

        const producer = await peer.getTransport(request.transportId()).produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
            appData: { source: request.source() },
        });

        this.announce(peer, room, producer, request.source());

        return new ProducerResource(producer);
    }

    /**
     * A transmissão chega como RTP puro, não por WebRTC. É assim que o app alcança mais
     * gente do que conexões diretas aguentam: continua codificando uma vez na GPU, mas
     * sobe uma vez só para o servidor, que replica.
     */
    public async storePlain(request: ProducePlainRequest): Promise<PlainProducerResource> {
        const peer = request.peer();
        const room = request.room();

        peer.assertCanProduce(request.source());

        const transport = await room.plainTransportFor(peer, request.srtpParameters());

        const producer = await transport.produce({
            kind: request.kind(),
            rtpParameters: request.rtpParameters(),
        });

        this.announce(peer, room, producer, request.source());

        return new PlainProducerResource(producer, transport);
    }

    /** Registra o producer e conta para a sala. É isto que acende o "ao vivo" dos outros. */
    private announce(peer: Peer, room: Room, producer: Producer, source: SourceName): void {
        peer.addProducer(producer, source);
        Recorder.follow(room.router, peer, producer);
        // Trinta segundos sem um pacote e o producer morre. Avisar quem transmite é o
        // ponto: `close` fala com a sala inteira MENOS o dono, então sem esta linha o app
        // segue mostrando "ao vivo" para sempre enquanto todo mundo vê tela preta. A
        // causa quase sempre é a porta de RTP deste worker fechada no firewall — a faixa
        // inteira precisa estar aberta, não só o começo dela.
        let idleTimer: ReturnType<typeof setTimeout> | undefined = setTimeout(() => {
            peer.send('producerDead', { producerId: producer.id, kind: producer.kind, source });
            this.close(peer, room, producer);
        }, MEDIA_IDLE_MS);

        producer.on('transportclose', () => {
            if (idleTimer) {
                clearTimeout(idleTimer);
            }
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

        // O producer é declarado antes de um único pacote chegar, então até o score subir
        // quem transmite não tem como distinguir "o servidor está recebendo" de "meus
        // pacotes não vão a lugar nenhum". Avisado uma vez: depois o score só oscila.
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

    public async pause(request: ProducerRequest): Promise<StatusResource> {
        const peer = request.peer();

        await request.room().setProducerPaused(peer, peer.getProducer(request.producerId()), true);

        return new StatusResource('paused');
    }

    public async resume(request: ProducerRequest): Promise<StatusResource> {
        const peer = request.peer();

        await request.room().setProducerPaused(peer, peer.getProducer(request.producerId()), false);

        return new StatusResource('resumed');
    }

    public destroy(request: ProducerRequest): StatusResource {
        this.close(
            request.peer(),
            request.room(),
            request.peer().producers.get(request.producerId()),
        );

        return new StatusResource('closed');
    }

    private close(peer: Peer, room: Room, producer: Producer | undefined): void {
        if (!producer || peer.producers.get(producer.id) !== producer) {
            return;
        }

        producer.close();
        peer.producers.delete(producer.id);
        room.broadcast(
            'producerClosed',
            {
                peerId: peer.id,
                producerId: producer.id,
                kind: producer.kind,
                source: String(producer.appData.source),
            },
            peer.id,
        );

        // Só o transport de RTP puro: o de WebRTC é do cliente, que produz de novo nele.
        if (peer.producers.size === 0) {
            peer.closePlainTransports();
        }
    }
}
