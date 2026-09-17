export class Field {
    static require(value: string, message: string): void {
        if (value.trim() === '') {
            throw new Error(message);
        }
    }

    static requireEmail(value: string): void {
        Field.require(value, 'Digite o e-mail.');

        if (! /^[^\s@]+@[^\s@]+$/.test(value.trim())) {
            throw new Error('Esse e-mail não parece válido.');
        }
    }
}
