import { ValidationException } from '../../Exceptions/ApiException.js';

export class Request {
    constructor(data, session) {
        this.data = data ?? {};
        this.session = session;
        this.validate();
    }

    validate() {}

    peer() {
        return this.session.peer;
    }

    room() {
        return this.session.room;
    }

    string(key) {
        const value = this.data[key];

        if (typeof value !== 'string' || value.trim() === '') {
            throw new ValidationException(`o campo ${key} é obrigatório`);
        }

        return value;
    }

    object(key) {
        const value = this.data[key];

        if (typeof value !== 'object' || value === null) {
            throw new ValidationException(`o campo ${key} é obrigatório`);
        }

        return value;
    }

    enum(key, allowed) {
        const value = this.string(key);

        if (! allowed.includes(value)) {
            throw new ValidationException(`o campo ${key} deve ser um de: ${allowed.join(', ')}`);
        }

        return value;
    }

    integerOrNull(key) {
        const value = this.data[key];

        return Number.isInteger(value) ? value : null;
    }
}
