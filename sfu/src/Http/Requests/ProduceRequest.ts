import type { MediaKind, RtpParameters } from 'mediasoup/types';

import { SOURCES, type SourceName } from '../../Enums/Source.js';
import { Request } from './Request.js';

export class ProduceRequest extends Request {
    protected override validate(): void {
        this.string('transportId');
        this.oneOf('kind', ['audio', 'video'] as const);
        this.oneOf('source', SOURCES);
        this.object('rtpParameters');
    }

    transportId(): string {
        return this.string('transportId');
    }

    kind(): MediaKind {
        return this.oneOf('kind', ['audio', 'video'] as const);
    }

    source(): SourceName {
        return this.oneOf('source', SOURCES);
    }

    rtpParameters(): RtpParameters {
        return this.object<RtpParameters>('rtpParameters');
    }
}
