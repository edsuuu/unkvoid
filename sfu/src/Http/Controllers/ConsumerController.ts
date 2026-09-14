import { ValidationException } from '../../Exceptions/ApiException.js';
import type { ConsumePlainRequest } from '../Requests/ConsumePlainRequest.js';
import type { ConsumeRequest } from '../Requests/ConsumeRequest.js';
import type { ConsumerRequest } from '../Requests/ConsumerRequest.js';
import { ConsumerResource } from '../Resources/ConsumerResource.js';
import { PlainConsumerResource } from '../Resources/PlainConsumerResource.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ConsumerController {
    public async store(request: ConsumeRequest): Promise<ConsumerResource> {
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

        // Nasce pausado de propósito: retomar só depois de o cliente confirmar o
        // consumer evita perder o primeiro keyframe e mostrar tela preta.
        const consumer = await peer.getTransport(request.transportId()).consume({
            producerId: request.producerId(),
            rtpCapabilities: request.rtpCapabilities(),
            paused: true,
        });

        peer.consumers.set(consumer.id, consumer);
        consumer.on('transportclose', () => peer.consumers.delete(consumer.id));
        consumer.on('producerclose', () => {
            peer.consumers.delete(consumer.id);
            peer.send('consumerClosed', {
                consumerId: consumer.id,
                producerId: consumer.producerId,
                kind: consumer.kind,
                // De quem era a tela, não de quem estava assistindo. Mandar o próprio id
                // fazia o cliente apagar o quadro errado — invisível enquanto ninguém
                // tinha quadro com o próprio id, e visível no instante em que passou a ter.
                peerId: owner.peer.id,
                source: String(owner.producer.appData.source),
            });
        });

        return new ConsumerResource(consumer, owner);
    }

    /**
     * O mesmo consumo, mas por RTP puro numa porta UDP — para o app que não tem
     * WebRTC na janela. As capacidades são as do próprio router: quem decodifica é o
     * GStreamer do lado de lá, que aceita o que o servidor tiver.
     */
    public async storePlain(request: ConsumePlainRequest): Promise<PlainConsumerResource> {
        const room = request.room();
        const peer = request.peer();
        const owner = room.findProducerOwner(request.producerId());
        const transport = await room.plainReceiveTransportFor(peer, request.srtpParameters());

        // Pausado como o outro: o cliente abre o caminho no roteador com o primeiro
        // pacote e só então pede para retomar, já com o keyframe junto.
        const consumer = await transport.consume({
            producerId: request.producerId(),
            rtpCapabilities: room.router.rtpCapabilities,
            paused: true,
        });

        peer.consumers.set(consumer.id, consumer);
        consumer.on('transportclose', () => peer.consumers.delete(consumer.id));
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

        return new PlainConsumerResource(consumer, transport, owner);
    }

    public async resume(request: ConsumerRequest): Promise<StatusResource> {
        const consumer = request.peer().getConsumer(request.consumerId());
        await consumer.resume();

        // Um consumer novo pode começar num quadro parcial. Pedir um IDR na hora evita
        // esperar o keyframe periódico do encoder e tira aquele atraso de "conectado
        // mas preto" logo depois de entrar ou reconectar.
        if (consumer.kind === 'video') {
            await consumer.requestKeyFrame();
        }

        return new StatusResource('resumed');
    }

    public async pause(request: ConsumerRequest): Promise<StatusResource> {
        await request.peer().getConsumer(request.consumerId()).pause();

        return new StatusResource('paused');
    }

    public destroy(request: ConsumerRequest): StatusResource {
        const peer = request.peer();

        peer.getConsumer(request.consumerId()).close();
        peer.consumers.delete(request.consumerId());

        return new StatusResource('closed');
    }
}
