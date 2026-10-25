import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';

export const E2EE_VERSION = 1;

/** MLS mensagens pequenas cabem com folga; o limite protege a fila e o heap do SFU. */
export const MAX_E2EE_PAYLOAD_BYTES = 64 * 1024;

const MAX_GROUP_BYTES = 128;

const BASE64URL = /^[A-Za-z0-9_-]+$/;

export type E2eeOpaquePayload = {
    version: number;
    group: string;
    payload: string;
    payloadBytes: number;
};

/** Campos comuns de blocos MLS. O SFU só valida a forma e os limites, nunca desserializa MLS. */
export class E2eeRequest extends Request {
    protected opaquePayload(): E2eeOpaquePayload {
        const version = this.data.version;

        if (version !== E2EE_VERSION) {
            throw new ValidationException('unsupported E2EE version');
        }

        const group = this.base64url('group', MAX_GROUP_BYTES);
        const payload = this.base64url('payload', MAX_E2EE_PAYLOAD_BYTES);

        return {
            version,
            group: group.value,
            payload: payload.value,
            payloadBytes: payload.bytes,
        };
    }

    protected epoch(): number {
        const epoch = this.data.epoch;

        if (!Number.isSafeInteger(epoch) || (epoch as number) < 0) {
            throw new ValidationException('field epoch must be a non-negative safe integer');
        }

        return epoch as number;
    }

    private base64url(key: string, maximumBytes: number): { value: string; bytes: number } {
        const value = this.string(key);

        if (!BASE64URL.test(value)) {
            throw new ValidationException(`field ${key} must be base64url without padding`);
        }

        if (value.length % 4 === 1) {
            throw new ValidationException(`field ${key} is not valid base64url`);
        }

        // This is the decoded length rounded up from the base64url representation. Checking it
        // before Buffer allocation keeps a hostile socket from making the SFU decode megabytes.
        const bytes = Math.floor((value.length * 3) / 4);

        if (bytes === 0 || bytes > maximumBytes) {
            throw new ValidationException(`field ${key} exceeds the E2EE size limit`);
        }

        return { value, bytes };
    }
}
