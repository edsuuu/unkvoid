import type { Producer } from 'mediasoup/types';

import type { SourceName } from '../../Enums/Source.js';
import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import type { ProducePlainRequest } from '../Requests/ProducePlainRequest.js';
import type { ProducerRequest } from '../Requests/ProducerRequest.js';
import { PlainProducerResource } from '../Resources/PlainProducerResource.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ProducerController {
    /**
     * A transmissão chega como RTP puro, não por WebRTC. É assim que o app alcança mais
     * gente do que conexões diretas aguentam: continua codificando uma vez na GPU, mas
     * sobe uma vez só para o servidor, que replica.
     */
    async storePlain(request: ProducePlainRequest): Promise<PlainProducerResource> {
        const peer = request.peer();
        const room = request.room();
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
        producer.on('transportclose', () => peer.producers.delete(producer.id));

        // O producer é declarado antes de um único pacote chegar, então até o score subir
        // quem transmite não tem como distinguir "o servidor está recebendo" de "meus
        // pacotes não vão a lugar nenhum". Avisado uma vez: depois o score só oscila.
        let receiving = false;

        producer.on('score', scores => {
            if (receiving || ! scores.some(entry => entry.score > 0)) {
                return;
            }

            receiving = true;
            peer.send('producerActive', { producerId: producer.id });
        });

        room.broadcast('newProducer', {
            peerId: peer.id,
            name: peer.name,
            producerId: producer.id,
            kind: producer.kind,
            source,
        }, peer.id);
    }

    destroy(request: ProducerRequest): StatusResource {
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
