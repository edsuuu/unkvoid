import { Request } from './Request.js';

export class ConsumeRequest extends Request {
    validate() {
        this.string('transportId');
        this.string('producerId');
        this.object('rtpCapabilities');
    }

    transportId() {
        return this.string('transportId');
    }

    producerId() {
        return this.string('producerId');
    }

    rtpCapabilities() {
        return this.object('rtpCapabilities');
    }
}
