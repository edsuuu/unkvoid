import { Request } from './Request.js';

export class JoinRequest extends Request {
    validate() {
        this.string('token');
    }

    token() {
        return this.string('token');
    }
}
