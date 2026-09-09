import type { MediaKind, RtpParameters, SrtpParameters } from 'mediasoup/types';

import { Request } from './Request.js';
import { SOURCES, type SourceName } from '../../Enums/Source.js';
import { ValidationException } from '../../Exceptions/ApiException.js';

const SUITES = [
    'AEAD_AES_256_GCM',
    'AEAD_AES_128_GCM',
    'AES_CM_128_HMAC_SHA1_80',
    'AES_CM_128_HMAC_SHA1_32',
] as const;

/**
 * Uma transmissão que chega como RTP puro, vinda do app nativo e não de um navegador.
 *
 * O cliente traz a própria chave SRTP: quem envia é ele, então é dele a chave que
 * protege o que sai. O servidor responde com a chave do sentido contrário.
 */
export class ProducePlainRequest extends Request {
    protected override validate(): void {
        this.oneOf('kind', ['audio', 'video'] as const);
        this.oneOf('source', SOURCES);
        this.object('rtpParameters');

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

    public kind(): MediaKind {
        return this.oneOf('kind', ['audio', 'video'] as const);
    }

    public source(): SourceName {
        return this.oneOf('source', SOURCES);
    }

    public rtpParameters(): RtpParameters {
        return this.object<RtpParameters>('rtpParameters');
    }

    public srtpParameters(): SrtpParameters {
        return this.object<SrtpParameters>('srtpParameters');
    }
}
