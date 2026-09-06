import { Request } from './Request.js';

export class ProducerRequest extends Request {
    validate() {
        this.string('producerId');
    }

    producerId() {
        return this.string('producerId');
    }
}
