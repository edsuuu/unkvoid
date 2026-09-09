import { Request } from './Request.js';

export class ConsumerRequest extends Request {
    protected override validate(): void {
        this.string('consumerId');
    }

    consumerId(): string {
        return this.string('consumerId');
    }
}
