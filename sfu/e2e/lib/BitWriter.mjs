/** Escreve o RBSP do cabeçalho de fatia que o harness monta para os quadros P. */
export class BitWriter {
    constructor() {
        this.bytes = [];
        this.current = 0;
        this.filled = 0;
    }

    bit(value) {
        this.current = (this.current << 1) | (value & 1);
        this.filled += 1;

        if (this.filled === 8) {
            this.bytes.push(this.current);
            this.current = 0;
            this.filled = 0;
        }
    }

    bits(value, count) {
        for (let index = count - 1; index >= 0; index -= 1) {
            this.bit(Math.floor(value / 2 ** index) & 1);
        }
    }

    unsigned(value) {
        const code = value + 1;
        const length = Math.floor(Math.log2(code));

        this.bits(0, length);
        this.bits(code, length + 1);
    }

    signed(value) {
        this.unsigned(value > 0 ? value * 2 - 1 : -value * 2);
    }

    /** O `rbsp_trailing_bits`: um bit 1 e zeros até fechar o byte. */
    finish() {
        this.bit(1);

        while (this.filled !== 0) {
            this.bit(0);
        }

        return Buffer.from(this.bytes);
    }
}
