import { Request } from './Request.js';

/**
 * Sinalização P2P. O SFU não entende o conteúdo: só entrega de um participante para
 * outro dentro da mesma sala. Assim o P2P reaproveita a sala e a autenticação que já
 * existem, em vez de exigir um segundo canal de sinalização.
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
