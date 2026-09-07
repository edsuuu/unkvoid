import { Request } from './Request.js';

export class JoinRequest extends Request {
    protected override validate(): void {
        this.string('token');
    }

    token(): string {
        return this.string('token');
    }

    /**
     * Only the client knows whether its transports are still alive. After F5 it is
     * brand new, so it requests a clean session even if the server still retains the
     * old one — resuming it would leave the client without any transports.
     */
    wantsResume(): boolean {
        return this.data.resume === true;
    }
}
