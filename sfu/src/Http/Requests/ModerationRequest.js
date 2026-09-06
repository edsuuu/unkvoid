import { Request } from './Request.js';

export class ModerationRequest extends Request {
    validate() {
        this.string('peerId');
    }

    targetPeerId() {
        return this.string('peerId');
    }
}
