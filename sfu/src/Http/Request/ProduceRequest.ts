import type { MediaKind, RtpParameters } from 'mediasoup/types';

import { Request } from './Request.js';
import { SOURCES, type SourceName } from '../../Enums/Source.js';

export class ProduceRequest extends Request {
    protected override validate(): void {
        this.string('transportId');
        this.oneOf('kind', ['audio', 'video'] as const);
        this.oneOf('source', SOURCES);
        this.object('rtpParameters');
    }

    public transportId(): string {
        return this.string('transportId');
    }

    public kind(): MediaKind {
        return this.oneOf('kind', ['audio', 'video'] as const);
    }

    public source(): SourceName {
        return this.oneOf('source', SOURCES);
    }

    public rtpParameters(): RtpParameters {
        return this.object<RtpParameters>('rtpParameters');
    }
}
