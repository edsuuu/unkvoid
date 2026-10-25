import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';

// `friend.ID` leva o status e a atividade de um amigo aceito; quem decide é o Laravel.
const CHANNEL = /^((channel|user|server|friend)\.[0-9A-Za-z]{1,32}|releases)$/;

export class SubscribeRequest extends Request {
    protected override validate(): void {
        if (!CHANNEL.test(this.string('channel'))) {
            throw new ValidationException(
                'field channel must be channel.ID, user.ID, server.ID, friend.ID or releases',
            );
        }
    }

    public channel(): string {
        return this.string('channel');
    }
}
