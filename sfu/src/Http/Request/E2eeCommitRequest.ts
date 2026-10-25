import { E2eeRequest, type E2eeOpaquePayload } from './E2eeRequest.js';

export type E2eeCommit = E2eeOpaquePayload & { epoch: number };

export class E2eeCommitRequest extends E2eeRequest {
    public commit(): E2eeCommit {
        return { ...this.opaquePayload(), epoch: this.epoch() };
    }
}
