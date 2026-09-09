import type { Consumer, Producer } from 'mediasoup/types';

import type { Peer } from '../../Services/Peer.js';
import type { Resource } from '../../types.js';

export class ConsumerResource implements Resource {
    public constructor(
        private readonly consumer: Consumer,
        private readonly owner: { peer: Peer; producer: Producer },
    ) {}

    public toArray(): Record<string, unknown> {
        return {
            consumerId: this.consumer.id,
            producerId: this.consumer.producerId,
            kind: this.consumer.kind,
            rtpParameters: this.consumer.rtpParameters,
            peerId: this.owner.peer.id,
            name: this.owner.peer.name,
            source: String(this.owner.producer.appData.source),
        };
    }
}
