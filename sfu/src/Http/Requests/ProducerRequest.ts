import { Request } from './Request.js';

export class ProducerRequest extends Request {
    protected override validate(): void {
        this.string('producerId');
    }

    producerId(): string {
        return this.string('producerId');
    }
}
