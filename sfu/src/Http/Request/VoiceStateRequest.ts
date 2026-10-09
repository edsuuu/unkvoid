import { Request } from './Request.js';

export class VoiceStateRequest extends Request {
    protected override validate(): void {
        this.boolean('muted');
        this.boolean('deafened');
    }

    public muted(): boolean {
        return this.boolean('muted');
    }

    public deafened(): boolean {
        return this.boolean('deafened');
    }
}
