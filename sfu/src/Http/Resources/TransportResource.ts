import type { WebRtcTransport } from 'mediasoup/types';

import type { Resource } from '../../types.js';

export class TransportResource implements Resource {
    public constructor(private readonly transport: WebRtcTransport) {}

    public toArray(): Record<string, unknown> {
        return {
            transportId: this.transport.id,
            iceParameters: this.transport.iceParameters,
            iceCandidates: this.transport.iceCandidates,
            dtlsParameters: this.transport.dtlsParameters,
        };
    }
}
