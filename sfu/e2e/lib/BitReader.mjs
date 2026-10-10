/** Lê o RBSP do H.264 bit a bit: os campos fixos e os Exp-Golomb do SPS e do PPS. */
export class BitReader {
    constructor(rbsp) {
        this.bytes = rbsp;
        this.position = 0;
    }

    bit() {
        if (this.position >= this.bytes.length * 8) {
            throw new Error('H.264 bitstream ended early');
        }

        const value = (this.bytes[this.position >> 3] >> (7 - (this.position & 7))) & 1;

        this.position += 1;

        return value;
    }

    bits(count) {
        let value = 0;

        for (let index = 0; index < count; index += 1) {
            value = value * 2 + this.bit();
        }

        return value;
    }

    unsigned() {
        let zeros = 0;

        while (this.bit() === 0) {
            zeros += 1;
        }

        return 2 ** zeros - 1 + this.bits(zeros);
    }

    signed() {
        const code = this.unsigned();

        return code % 2 === 1 ? (code + 1) / 2 : -code / 2;
    }

    skip(count) {
        this.position += count;
    }
}
