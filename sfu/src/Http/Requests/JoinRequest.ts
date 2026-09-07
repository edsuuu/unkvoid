import { Request } from './Request.js';

export class JoinRequest extends Request {
    protected override validate(): void {
        this.string('token');
    }

    token(): string {
        return this.string('token');
    }
}
