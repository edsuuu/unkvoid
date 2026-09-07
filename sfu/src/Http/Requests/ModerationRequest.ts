import { Request } from './Request.js';

export class ModerationRequest extends Request {
    protected override validate(): void {
        this.string('peerId');
    }

    targetPeerId(): string {
        return this.string('peerId');
    }
}
