import { E2eeRequest, type E2eeOpaquePayload } from './E2eeRequest.js';

export type E2eeWelcome = E2eeOpaquePayload & { epoch: number; targetPeerId: string };

export class E2eeWelcomeRequest extends E2eeRequest {
    public welcome(): E2eeWelcome {
        return {
            ...this.opaquePayload(),
            epoch: this.epoch(),
            targetPeerId: this.string('targetPeerId'),
        };
    }
}
