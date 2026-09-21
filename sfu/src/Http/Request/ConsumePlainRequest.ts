import type { SrtpParameters } from 'mediasoup/types';

import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';

const SUITES = ['AES_CM_128_HMAC_SHA1_80'] as const;

export class ConsumePlainRequest extends Request {
    protected override validate(): void {
        this.string('producerId');

        const srtp = this.object<Record<string, unknown>>('srtpParameters');

        if (!(SUITES as readonly unknown[]).includes(srtp.cryptoSuite)) {
            throw new ValidationException(
                `field srtpParameters.cryptoSuite must be one of: ${SUITES.join(', ')}`,
            );
        }

        if (typeof srtp.keyBase64 !== 'string' || srtp.keyBase64.trim() === '') {
            throw new ValidationException('field srtpParameters.keyBase64 is required');
        }
    }

    public producerId(): string {
        return this.string('producerId');
    }

    public srtpParameters(): SrtpParameters {
        return this.object<SrtpParameters>('srtpParameters');
    }
}
