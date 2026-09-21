import { Request } from './Request.js';
import { Signature, type SessionClaims } from '../../Services/Signature.js';

export class IdentifyRequest extends Request {
    protected override validate(): void {
        this.string('token');
    }

    public claims(): SessionClaims {
        return Signature.sessionClaims(this.string('token'));
    }
}
