import { Request } from './Request.js';

export class TransportRequest extends Request {
    validate() {
        this.string('transportId');
        this.object('dtlsParameters');
    }

    transportId() {
        return this.string('transportId');
    }

    dtlsParameters() {
        return this.object('dtlsParameters');
    }
}
