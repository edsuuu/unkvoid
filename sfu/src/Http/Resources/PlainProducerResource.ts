import type { PlainTransport, Producer } from 'mediasoup/types';

import type { Resource } from '../../types.js';

/**
 * Para onde mandar o RTP, e com que chave ele volta protegido. O endereço é o que o
 * app nativo precisa; o resto da transmissão ele já sabe, porque foi ele que escolheu
 * o SSRC e o tipo de payload.
 */
export class PlainProducerResource implements Resource {
    public constructor(
        private readonly producer: Producer,
        private readonly transport: PlainTransport,
    ) {}

    public toArray(): Record<string, unknown> {
        return {
            producerId: this.producer.id,
            kind: this.producer.kind,
            source: String(this.producer.appData.source),
            ip: this.transport.tuple.localAddress,
            port: this.transport.tuple.localPort,
            srtpParameters: this.transport.srtpParameters,
        };
    }
}
