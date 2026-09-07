import { Request } from './Request.js';

export class ConsumerRequest extends Request {
    protected override validate(): void {
        this.string('consumerId');
    }

    consumerId(): string {
        return this.string('consumerId');
    }

    spatialLayer(): number {
        return this.integerOrNull('spatialLayer') ?? 2;
    }

    temporalLayer(): number | undefined {
        return this.integerOrNull('temporalLayer');
    }
}
