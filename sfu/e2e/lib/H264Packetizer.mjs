import { NAL, nalType } from './h264.mjs';

/** O MTU do app (1200) menos os 12 bytes do cabeçalho RTP, como o `new_packetizer` faz. */
const MOST_PAYLOAD = 1188;

/**
 * O empacotamento do `H264Payloader` que o app usa: SPS e PPS juntos num STAP-A antes do
 * NAL seguinte, NAL que cabe vai sozinho, NAL maior vira FU-A. A única diferença é o
 * enchimento (NAL 12), que o app nunca gera e aqui sobe em FU-A como qualquer outro.
 */
export class H264Packetizer {
    constructor() {
        this.sps = null;
        this.pps = null;
    }

    payloads(nals) {
        const output = [];

        for (const nal of nals) {
            const type = nalType(nal);

            if (type === NAL.SPS) {
                this.sps = nal;
                continue;
            }

            if (type === NAL.PPS) {
                this.pps = nal;
                continue;
            }

            if (this.sps && this.pps) {
                output.push(this.aggregate(this.sps, this.pps));
                this.sps = null;
                this.pps = null;
            }

            if (nal.length <= MOST_PAYLOAD) {
                output.push(nal);
                continue;
            }

            output.push(...this.fragment(nal));
        }

        return output;
    }

    aggregate(...nals) {
        const parts = [Buffer.from([(nals[0][0] & 0x60) | NAL.STAP_A])];

        for (const nal of nals) {
            const size = Buffer.alloc(2);

            size.writeUInt16BE(nal.length);
            parts.push(size, nal);
        }

        return Buffer.concat(parts);
    }

    fragment(nal) {
        const indicator = (nal[0] & 0xe0) | NAL.FU_A;
        const type = nal[0] & 0x1f;
        const fragments = [];
        const room = MOST_PAYLOAD - 2;

        for (let offset = 1; offset < nal.length; offset += room) {
            const end = Math.min(offset + room, nal.length);
            const start = offset === 1 ? 0x80 : 0;
            const last = end === nal.length ? 0x40 : 0;

            fragments.push(Buffer.concat([Buffer.from([indicator, start | last | type]), nal.subarray(offset, end)]));
        }

        return fragments;
    }
}
