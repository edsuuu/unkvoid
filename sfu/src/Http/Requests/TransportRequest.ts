import type { DtlsParameters } from 'mediasoup/types';

import { Request } from './Request.js';

export class TransportRequest extends Request {
    protected override validate(): void {
        this.string('transportId');
        this.object('dtlsParameters');
    }

    public transportId(): string {
        return this.string('transportId');
    }

    public dtlsParameters(): DtlsParameters {
        return this.object<DtlsParameters>('dtlsParameters');
    }
}
