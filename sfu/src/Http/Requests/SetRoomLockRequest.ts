import { Request } from './Request.js';

export class SetRoomLockRequest extends Request {
    public locked(): boolean {
        return this.boolean('locked');
    }
}
