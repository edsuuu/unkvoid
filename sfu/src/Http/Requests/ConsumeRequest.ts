import type { RtpCapabilities } from 'mediasoup/types';

import { Request } from './Request.js';

export class ConsumeRequest extends Request {
    protected override validate(): void {
        this.string('transportId');
        this.string('producerId');
        this.object('rtpCapabilities');
    }

    transportId(): string {
        return this.string('transportId');
    }

    producerId(): string {
        return this.string('producerId');
    }

    rtpCapabilities(): RtpCapabilities {
        return this.object<RtpCapabilities>('rtpCapabilities');
    }
}
