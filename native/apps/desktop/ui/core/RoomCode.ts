export class RoomCode {
    static readonly LENGTH = 12;
    static readonly MIN_LENGTH = 3;
    static readonly MAX_LENGTH = 32;

    private static readonly ALPHABET = 'abcdefghijklmnopqrstuvwxyz0123456789';
    private static readonly UNBIASED_BYTE_LIMIT = RoomCode.ALPHABET.length * Math.floor(256 / RoomCode.ALPHABET.length);

    static isValid(value: unknown): value is string {
        return typeof value === 'string'
            && value.length >= RoomCode.MIN_LENGTH
            && value.length <= RoomCode.MAX_LENGTH
            && /^[a-z0-9][a-z0-9-]*[a-z0-9]$/.test(value);
    }

    static generate(): string {
        const code: string[] = [];

        while (code.length < RoomCode.LENGTH) {
            for (const byte of crypto.getRandomValues(new Uint8Array(RoomCode.LENGTH))) {
                if (byte < RoomCode.UNBIASED_BYTE_LIMIT && code.length < RoomCode.LENGTH) {
                    code.push(RoomCode.ALPHABET[byte % RoomCode.ALPHABET.length]);
                }
            }
        }

        return code.join('');
    }
}
