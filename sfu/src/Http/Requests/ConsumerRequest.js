import { Request } from './Request.js';

export class ConsumerRequest extends Request {
    validate() {
        this.string('consumerId');
    }

    consumerId() {
        return this.string('consumerId');
    }

    spatialLayer() {
        return this.integerOrNull('spatialLayer');
    }

    temporalLayer() {
        return this.integerOrNull('temporalLayer');
    }
}
