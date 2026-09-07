import type { MediaKind, RtpParameters, SrtpParameters } from 'mediasoup/types';

import { SOURCES, type SourceName } from '../../Enums/Source.js';
import { ValidationException } from '../../Exceptions/ApiException.js';
import { Request } from './Request.js';

const SUITES = ['AEAD_AES_256_GCM', 'AEAD_AES_128_GCM', 'AES_CM_128_HMAC_SHA1_80', 'AES_CM_128_HMAC_SHA1_32'] as const;

/**
 * A broadcast arriving as plain RTP, from the native app rather than a browser.
 *
 * The client brings its own SRTP key: it is the sender, so it owns the key that
 * protects what it sends. The server answers with the key for the other direction.
 */
export class ProducePlainRequest extends Request {
    protected override validate(): void {
        this.oneOf('kind', ['audio', 'video'] as const);
        this.oneOf('source', SOURCES);
        this.object('rtpParameters');

        const srtp = this.object<Record<string, unknown>>('srtpParameters');

        if (! (SUITES as readonly unknown[]).includes(srtp.cryptoSuite)) {
            throw new ValidationException(`field srtpParameters.cryptoSuite must be one of: ${SUITES.join(', ')}`);
        }

        if (typeof srtp.keyBase64 !== 'string' || srtp.keyBase64.trim() === '') {
            throw new ValidationException('field srtpParameters.keyBase64 is required');
        }
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

    srtpParameters(): SrtpParameters {
        return this.object<SrtpParameters>('srtpParameters');
    }
}
