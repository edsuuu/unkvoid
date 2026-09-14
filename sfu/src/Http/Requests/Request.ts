import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import type { Session } from '../../types.js';

export class Request {
    protected readonly data: Record<string, unknown>;

    public constructor(
        data: Record<string, unknown> | undefined,
        public readonly session: Session,
    ) {
        this.data = data ?? {};
        this.validate();
    }

    protected validate(): void {}

    public peer(): Peer {
        if (!this.session.peer) {
            throw new ValidationException('this action requires a room');
        }

        return this.session.peer;
    }

    public room(): Room {
        if (!this.session.room) {
            throw new ValidationException('this action requires a room');
        }

        return this.session.room;
    }

    protected string(key: string): string {
        const value = this.data[key];

        if (typeof value !== 'string' || value.trim() === '') {
            throw new ValidationException(`field ${key} is required`);
        }

        return value;
    }

    protected object<T extends object>(key: string): T {
        const value = this.data[key];

        if (typeof value !== 'object' || value === null || Array.isArray(value)) {
            throw new ValidationException(`field ${key} is required`);
        }

        return value as T;
    }

    protected oneOf<T extends string>(key: string, allowed: readonly T[]): T {
        const value = this.string(key);

        if (!(allowed as readonly string[]).includes(value)) {
            throw new ValidationException(`field ${key} must be one of: ${allowed.join(', ')}`);
        }

        return value as T;
    }

    protected boolean(key: string): boolean {
        const value = this.data[key];

        if (typeof value !== 'boolean') {
            throw new ValidationException(`field ${key} must be true or false`);
        }

        return value;
    }
}
