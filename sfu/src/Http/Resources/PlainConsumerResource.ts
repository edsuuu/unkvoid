import type { Consumer, PlainTransport, Producer } from 'mediasoup/types';

import type { Peer } from '../../Services/Peer.js';
import type { Resource } from '../../types.js';

/**
 * De onde a mídia vai chegar e como abri-la: a porta do servidor, a chave com que ele
 * protege o que manda, e o tipo de payload que o decodificador precisa reconhecer.
 */
export class PlainConsumerResource implements Resource {
    public constructor(
        private readonly consumer: Consumer,
        private readonly transport: PlainTransport,
        private readonly owner: { peer: Peer; producer: Producer },
    ) {}

    public toArray(): Record<string, unknown> {
        const codec = this.consumer.rtpParameters.codecs[0];

        return {
            consumerId: this.consumer.id,
            producerId: this.consumer.producerId,
            kind: this.consumer.kind,
            payloadType: codec?.payloadType ?? null,
            clockRate: codec?.clockRate ?? null,
            ip: this.transport.tuple.localAddress,
            port: this.transport.tuple.localPort,
            srtpParameters: this.transport.srtpParameters,
            peerId: this.owner.peer.id,
            name: this.owner.peer.name,
            source: String(this.owner.producer.appData.source),
        };
    }
}
