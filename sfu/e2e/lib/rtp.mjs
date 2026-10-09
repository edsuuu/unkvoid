/**
 * Os pacotes RTP e RTCP que o app nativo monta à mão (`media/src/plain.rs`,
 * `receiver.rs`, `recovery.rs`): o cabeçalho, o relatório do remetente, o NACK, o PLI e o
 * desembrulho do RTX.
 */

export const PAYLOAD_VIDEO = 96;
export const PAYLOAD_AUDIO = 111;
export const VIDEO_CLOCK = 90_000;
export const AUDIO_CLOCK = 48_000;

const RTCP_SR = 200;
const RTCP_RTPFB = 205;
const RTCP_PSFB = 206;
const RTCP_LEGACY_FIR = 192;
const GENERIC_NACK = 1;
const PLI = 1;
const FIR = 4;

/** Um SSRC por origem, a partir da base sorteada por transmissão (`Source::ssrc`). */
export const SOURCE_OFFSET = { screen: 0, screenAudio: 1, camera: 2, mic: 3 };

export const isVideoSource = source => source === 'screen' || source === 'camera';

export const buildRtp = ({ payloadType, sequence, timestamp, ssrc, marker, payload }) => {
    const packet = Buffer.allocUnsafe(12 + payload.length);

    packet[0] = 0x80;
    packet[1] = (marker ? 0x80 : 0) | payloadType;
    packet.writeUInt16BE(sequence & 0xffff, 2);
    packet.writeUInt32BE(timestamp >>> 0, 4);
    packet.writeUInt32BE(ssrc >>> 0, 8);
    payload.copy(packet, 12);

    return packet;
};

export const isRtcp = datagram => datagram.length >= 2 && datagram[1] >= 192 && datagram[1] <= 223;

/** O cabeçalho em claro, que o SRTP não cifra: é o que basta para medir perda e quadro. */
export const parseRtp = packet => {
    if (packet.length < 12 || packet[0] >> 6 !== 2) {
        return null;
    }

    let offset = 12 + (packet[0] & 0x0f) * 4;

    if (packet[0] & 0x10) {
        if (packet.length < offset + 4) {
            return null;
        }

        offset += 4 + packet.readUInt16BE(offset + 2) * 4;
    }

    let end = packet.length;

    if (packet[0] & 0x20) {
        end -= packet[packet.length - 1];
    }

    if (offset > end) {
        return null;
    }

    return {
        marker: (packet[1] & 0x80) !== 0,
        payloadType: packet[1] & 0x7f,
        sequence: packet.readUInt16BE(2),
        timestamp: packet.readUInt32BE(4),
        ssrc: packet.readUInt32BE(8),
        headerLength: offset,
        payload: packet.subarray(offset, end),
    };
};

/** O relatório do remetente, uma vez por segundo por origem: sem ele o consumer não anda. */
export const buildSenderReport = ({ ssrc, timestamp, packets, bytes }) => {
    const packet = Buffer.alloc(28);
    const now = Date.now();
    const seconds = Math.floor(now / 1000) + 2_208_988_800;
    const fraction = Math.floor(((now % 1000) / 1000) * 4_294_967_296);

    packet[0] = 0x80;
    packet[1] = RTCP_SR;
    packet.writeUInt16BE(6, 2);
    packet.writeUInt32BE(ssrc >>> 0, 4);
    packet.writeUInt32BE(seconds >>> 0, 8);
    packet.writeUInt32BE(fraction >>> 0, 12);
    packet.writeUInt32BE(timestamp >>> 0, 16);
    packet.writeUInt32BE(packets >>> 0, 20);
    packet.writeUInt32BE(bytes >>> 0, 24);

    return packet;
};

/**
 * O relatório de recepção (RR) com o LSR e o DLSR do último SR de cada fluxo: é com ele que o
 * mediasoup mede a ida e volta até quem assiste. Sem ele o consumer fica com 100 ms fixos.
 */
export const buildReceiverReport = (sender, blocks) => {
    const packet = Buffer.alloc(8 + blocks.length * 24);

    packet[0] = 0x80 | blocks.length;
    packet[1] = 201;
    packet.writeUInt16BE(1 + blocks.length * 6, 2);
    packet.writeUInt32BE(sender >>> 0, 4);

    blocks.forEach((block, index) => {
        const offset = 8 + index * 24;

        packet.writeUInt32BE(block.ssrc >>> 0, offset);
        packet.writeUInt32BE(block.highest >>> 0, offset + 8);
        packet.writeUInt32BE(block.lastSr >>> 0, offset + 16);
        packet.writeUInt32BE(block.delaySinceLastSr >>> 0, offset + 20);
    });

    return packet;
};

/** Os SR de um RTCP composto: o SSRC e o meio do relógio NTP (o `LSR` do RR). */
export const readSenderReports = rtcp => {
    const reports = [];
    let rest = rtcp;

    while (rest.length >= 4) {
        const size = (rest.readUInt16BE(2) + 1) * 4;

        if (size > rest.length) {
            break;
        }

        if (rest[1] === RTCP_SR && size >= 28) {
            reports.push({ ssrc: rest.readUInt32BE(4), middle: ((rest.readUInt32BE(8) & 0xffff) << 16 | rest.readUInt32BE(12) >>> 16) >>> 0 });
        }

        rest = rest.subarray(size);
    }

    return reports;
};

/** O RRTR (XR, bloco 4) que o mediasoup manda a quem transmite: o meio do relógio NTP dele. */
export const readReferenceTimes = rtcp => {
    const times = [];
    let rest = rtcp;

    while (rest.length >= 4) {
        const size = (rest.readUInt16BE(2) + 1) * 4;

        if (size > rest.length) {
            break;
        }

        if (rest[1] === 207) {
            for (let offset = 8; offset + 4 <= size; ) {
                const type = rest[offset];
                const words = rest.readUInt16BE(offset + 2);

                if (type === 4 && words === 2 && offset + 12 <= size) {
                    times.push(((rest.readUInt32BE(offset + 4) & 0xffff) << 16 | rest.readUInt32BE(offset + 8) >>> 16) >>> 0);
                }

                offset += 4 + words * 4;
            }
        }

        rest = rest.subarray(size);
    }

    return times;
};

/** O XR DLRR (bloco 5) de volta: é por ele que o mediasoup mede a ida e volta da subida. */
export const buildDelaySinceLastReferenceTime = (ssrc, lastReference, delay) => {
    const packet = Buffer.alloc(24);

    packet[0] = 0x80;
    packet[1] = 207;
    packet.writeUInt16BE(5, 2);
    packet.writeUInt32BE(ssrc >>> 0, 4);
    packet[8] = 5;
    packet.writeUInt16BE(3, 10);
    packet.writeUInt32BE(ssrc >>> 0, 12);
    packet.writeUInt32BE(lastReference >>> 0, 16);
    packet.writeUInt32BE(delay >>> 0, 20);

    return packet;
};

/** O NACK genérico com os números agrupados de 17 em 17, como o `recovery::nack`. */
export const buildNack = (sender, media, sequences) => {
    const sorted = [...new Set(sequences)].sort((left, right) => left - right);
    const fields = [];

    for (const sequence of sorted) {
        const last = fields.at(-1);
        const distance = last ? (sequence - last.first) & 0xffff : 0;

        if (last && distance >= 1 && distance <= 16) {
            last.mask |= 1 << (distance - 1);
        } else {
            fields.push({ first: sequence, mask: 0 });
        }
    }

    const packet = Buffer.alloc(12 + fields.length * 4);

    packet[0] = 0x80 | GENERIC_NACK;
    packet[1] = RTCP_RTPFB;
    packet.writeUInt16BE(2 + fields.length, 2);
    packet.writeUInt32BE(sender >>> 0, 4);
    packet.writeUInt32BE(media >>> 0, 8);

    fields.forEach(({ first, mask }, index) => {
        packet.writeUInt16BE(first, 12 + index * 4);
        packet.writeUInt16BE(mask, 14 + index * 4);
    });

    return packet;
};

export const buildPli = (sender, media) => {
    const packet = Buffer.alloc(12);

    packet[0] = 0x80 | PLI;
    packet[1] = RTCP_PSFB;
    packet.writeUInt16BE(2, 2);
    packet.writeUInt32BE(sender >>> 0, 4);
    packet.writeUInt32BE(media >>> 0, 8);

    return packet;
};

/** O que um RTCP composto pede a quem transmite: quadro-chave e reenvio, por SSRC. */
export const readFeedback = rtcp => {
    const keyframes = new Set();
    const nacks = [];
    let rest = rtcp;

    while (rest.length >= 4) {
        const format = rest[0] & 0x1f;
        const type = rest[1];
        const size = (rest.readUInt16BE(2) + 1) * 4;

        if (size > rest.length) {
            break;
        }

        const packet = rest.subarray(0, size);

        if (type === RTCP_LEGACY_FIR && size >= 8) {
            keyframes.add(packet.readUInt32BE(4));
        }

        if (type === RTCP_PSFB && format === PLI && size >= 12) {
            keyframes.add(packet.readUInt32BE(8));
        }

        if (type === RTCP_PSFB && format === FIR && size >= 20) {
            keyframes.add(packet.readUInt32BE(12));
        }

        if (type === RTCP_RTPFB && format === GENERIC_NACK && size >= 16) {
            const media = packet.readUInt32BE(8);

            for (let offset = 12; offset + 4 <= size; offset += 4) {
                const first = packet.readUInt16BE(offset);
                const mask = packet.readUInt16BE(offset + 2);

                nacks.push({ ssrc: media, sequence: first });

                for (let bit = 0; bit < 16; bit += 1) {
                    if (mask & (1 << bit)) {
                        nacks.push({ ssrc: media, sequence: (first + bit + 1) & 0xffff });
                    }
                }
            }
        }

        rest = rest.subarray(size);
    }

    return { keyframes, nacks };
};

/** O RTX (RFC 4588) volta a ser o pacote original: o número de verdade abre o payload. */
export const unwrapRtx = (packet, parsed, ssrc, payloadType) => {
    if (parsed.payload.length < 2) {
        return null;
    }

    const original = Buffer.allocUnsafe(parsed.headerLength + parsed.payload.length - 2);

    packet.copy(original, 0, 0, parsed.headerLength);
    original[0] &= ~0x20;
    original[1] = (packet[1] & 0x80) | (payloadType & 0x7f);
    parsed.payload.copy(original, 2, 0, 2);
    original.writeUInt32BE(ssrc >>> 0, 8);
    parsed.payload.copy(original, parsed.headerLength, 2);

    return original;
};

/** O número de 16 bits estendido perto de `near`, para passar do 65535 ao 0 sem voltar. */
export const extendSequence = (sequence, near) => {
    if (near === null || near === undefined) {
        return 2 ** 32 + sequence;
    }

    const base = near - (near % 65536);
    const candidates = [base - 65536 + sequence, base + sequence, base + 65536 + sequence];

    return candidates.reduce((best, candidate) =>
        Math.abs(candidate - near) < Math.abs(best - near) ? candidate : best,
    );
};
