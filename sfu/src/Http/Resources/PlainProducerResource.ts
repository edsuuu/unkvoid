import type { PlainTransport, Producer } from 'mediasoup/types';

import type { Resource } from '../../types.js';

/**
 * Where to send the RTP, and with which key it comes back protected. The address is
 * what the native app needs; everything else about the broadcast it already knows,
 * because it chose the SSRC and the payload type itself.
 */
export class PlainProducerResource implements Resource {
    constructor(
        private readonly producer: Producer,
        private readonly transport: PlainTransport,
    ) {}

    toArray(): Record<string, unknown> {
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
