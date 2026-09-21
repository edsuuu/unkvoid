import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';

const CHANNEL = /^(channel|user|server)\.[0-9A-Za-z]{1,32}$/;

export class SubscribeRequest extends Request {
    protected override validate(): void {
        if (!CHANNEL.test(this.string('channel'))) {
            throw new ValidationException('field channel must be channel.ID, user.ID or server.ID');
        }
    }

    public channel(): string {
        return this.string('channel');
    }
}
