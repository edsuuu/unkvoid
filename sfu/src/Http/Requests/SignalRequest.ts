import { Request } from './Request.js';

/**
 * P2P signaling. The SFU does not interpret the content; it only delivers from one participant to
 * another within the same room. This lets P2P reuse the room and its existing authentication
 * instead of requiring a second signaling channel.
 */
export class SignalRequest extends Request {
    protected override validate(): void {
        this.string('to');
        this.oneOf('kind', ['offer', 'answer', 'candidate'] as const);
        this.object('payload');
    }

    to(): string {
        return this.string('to');
    }

    kind(): 'offer' | 'answer' | 'candidate' {
        return this.oneOf('kind', ['offer', 'answer', 'candidate'] as const);
    }

    payload(): Record<string, unknown> {
        return this.object('payload');
    }
}
