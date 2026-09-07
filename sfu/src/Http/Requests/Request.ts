import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import type { Session } from '../../types.js';

export class Request {
    protected readonly data: Record<string, unknown>;

    constructor(data: Record<string, unknown> | undefined, public readonly session: Session) {
        this.data = data ?? {};
        this.validate();
    }

    protected validate(): void {}

    peer(): Peer {
        if (! this.session.peer) {
            throw new ValidationException('esta ação exige uma sala');
        }

        return this.session.peer;
    }

    room(): Room {
        if (! this.session.room) {
            throw new ValidationException('esta ação exige uma sala');
        }

        return this.session.room;
    }

    protected string(key: string): string {
        const value = this.data[key];

        if (typeof value !== 'string' || value.trim() === '') {
            throw new ValidationException(`o campo ${key} é obrigatório`);
        }

        return value;
    }

    protected object<T extends object>(key: string): T {
        const value = this.data[key];

        if (typeof value !== 'object' || value === null) {
            throw new ValidationException(`o campo ${key} é obrigatório`);
        }

        return value as T;
    }

    protected oneOf<T extends string>(key: string, allowed: readonly T[]): T {
        const value = this.string(key);

        if (! (allowed as readonly string[]).includes(value)) {
            throw new ValidationException(`o campo ${key} deve ser um de: ${allowed.join(', ')}`);
        }

        return value as T;
    }

    protected integerOrNull(key: string): number | undefined {
        const value = this.data[key];

        return Number.isInteger(value) ? (value as number) : undefined;
    }
}
