import { E2eeRequest, type E2eeOpaquePayload } from './E2eeRequest.js';

/** A request itself is the delivery wrapper for the new member's opaque MLS KeyPackage. */
export class E2eeJoinRequest extends E2eeRequest {
    public signal(): E2eeOpaquePayload {
        return this.opaquePayload();
    }
}
