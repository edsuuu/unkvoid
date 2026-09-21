import { randomUUID } from 'node:crypto';

import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';
import { Signature, type JoinClaims } from '../../Services/Signature.js';

const LEGACY_CODE = /^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$/;

const INSTALL_ID = /^[A-Za-z0-9-]{1,64}$/;

export class JoinRequest extends Request {
    declare private claims: JoinClaims;

    protected override validate(): void {
        if (typeof this.data.token === 'string') {
            this.claims = Signature.claims(this.data.token);

            return;
        }

        const room = this.string('room');
        const name = this.string('name').trim();

        if (room.length === 26 || !LEGACY_CODE.test(room) || name === '' || name.length > 40) {
            throw new ValidationException('field token is required');
        }

        const installId =
            typeof this.data.installId === 'string' && INSTALL_ID.test(this.data.installId)
                ? this.data.installId
                : randomUUID();

        this.claims = {
            room,
            sub: `guest:${installId}`,
            name,
            exp: 0,
            can: ['speak', 'stream', 'video'],
        };
    }

    public roomCode(): string {
        return this.claims.room;
    }

    public name(): string {
        return this.claims.name.trim();
    }

    public userId(): string {
        return this.claims.sub;
    }

    public can(): string[] {
        return this.claims.can;
    }

    public resumeKey(): string | null {
        return typeof this.data.resumeKey === 'string' ? this.data.resumeKey : null;
    }

    public wantsResume(): boolean {
        return this.data.resume === true;
    }
}
