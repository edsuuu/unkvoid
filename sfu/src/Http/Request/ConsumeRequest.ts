import type { RtpCapabilities } from 'mediasoup/types';

import { Request } from './Request.js';

export class ConsumeRequest extends Request {
    protected override validate(): void {
        this.string('transportId');
        this.string('producerId');
        this.object('rtpCapabilities');
    }

    public transportId(): string {
        return this.string('transportId');
    }

    public producerId(): string {
        return this.string('producerId');
    }

    public rtpCapabilities(): RtpCapabilities {
        return this.object<RtpCapabilities>('rtpCapabilities');
    }
}
