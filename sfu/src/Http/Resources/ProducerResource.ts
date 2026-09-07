import type { Producer } from 'mediasoup/types';

import type { Resource } from '../../types.js';

export class ProducerResource implements Resource {
    constructor(private readonly producer: Producer) {}

    toArray(): Record<string, unknown> {
        return {
            producerId: this.producer.id,
            kind: this.producer.kind,
            source: String(this.producer.appData.source),
        };
    }
}
