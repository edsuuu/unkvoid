import { createCipheriv, createHmac, timingSafeEqual } from 'node:crypto';

const KEY_LENGTH = 16;
const SALT_LENGTH = 14;
const AUTH_KEY_LENGTH = 20;
const TAG_LENGTH = 10;

/**
 * O SRTP do app nativo (`AES_CM_128_HMAC_SHA1_80`, RFC 3711), escrito à mão para o harness
 * falar com o mediasoup sem libsrtp: a chave mestra de 30 bytes (16 de chave e 14 de sal) é
 * a mesma `keyBase64` que vai no `producePlain` e no `consumePlain`.
 *
 * Um contexto cifra (o que sai com a nossa chave) ou decifra (o que chega com a do
 * servidor); o app usa dois, um de cada lado.
 */
export class SrtpSession {
    constructor(masterKeyAndSalt) {
        const master = Buffer.from(masterKeyAndSalt);

        if (master.length !== KEY_LENGTH + SALT_LENGTH) {
            throw new Error(`SRTP master key must be ${KEY_LENGTH + SALT_LENGTH} bytes`);
        }

        const key = master.subarray(0, KEY_LENGTH);
        const salt = master.subarray(KEY_LENGTH);

        this.rtpKey = derive(key, salt, 0x00, KEY_LENGTH);
        this.rtpAuth = derive(key, salt, 0x01, AUTH_KEY_LENGTH);
        this.rtpSalt = derive(key, salt, 0x02, SALT_LENGTH);
        this.rtcpKey = derive(key, salt, 0x03, KEY_LENGTH);
        this.rtcpAuth = derive(key, salt, 0x04, AUTH_KEY_LENGTH);
        this.rtcpSalt = derive(key, salt, 0x05, SALT_LENGTH);

        this.outgoing = new Map();
        this.incoming = new Map();
        this.rtcpIndex = 0;
    }

    encryptRtp(packet) {
        const header = headerLength(packet);
        const ssrc = packet.readUInt32BE(8);
        const sequence = packet.readUInt16BE(2);
        const state = this.outgoing.get(ssrc) ?? { last: sequence, roc: 0 };

        if (sequence < state.last && state.last - sequence > 0x8000) {
            state.roc = (state.roc + 1) >>> 0;
        }

        state.last = sequence;
        this.outgoing.set(ssrc, state);

        const output = Buffer.allocUnsafe(packet.length + TAG_LENGTH);

        packet.copy(output, 0, 0, header);
        ctr(this.rtpKey, rtpIv(this.rtpSalt, ssrc, state.roc, sequence), packet.subarray(header)).copy(output, header);

        const roc = Buffer.allocUnsafe(4);

        roc.writeUInt32BE(state.roc);
        tag(this.rtpAuth, output.subarray(0, packet.length), roc).copy(output, packet.length);

        return output;
    }

    /** `null` quando a etiqueta não confere: chave errada, ou ruído. */
    decryptRtp(packet, { verify = true } = {}) {
        if (packet.length < 12 + TAG_LENGTH) {
            return null;
        }

        const body = packet.subarray(0, packet.length - TAG_LENGTH);
        const header = headerLength(body);

        if (header > body.length) {
            return null;
        }

        const ssrc = body.readUInt32BE(8);
        const sequence = body.readUInt16BE(2);
        const state = this.incoming.get(ssrc) ?? { highest: sequence, roc: 0 };
        const roc = estimateRoc(state, sequence);

        if (verify) {
            const rocBytes = Buffer.allocUnsafe(4);

            rocBytes.writeUInt32BE(roc);

            if (!equal(tag(this.rtpAuth, body, rocBytes), packet.subarray(body.length))) {
                return null;
            }
        }

        if (roc === ((state.roc + 1) >>> 0)) {
            state.roc = roc;
            state.highest = sequence;
        } else if (roc === state.roc && sequence > state.highest) {
            state.highest = sequence;
        }

        this.incoming.set(ssrc, state);

        const output = Buffer.allocUnsafe(body.length);

        body.copy(output, 0, 0, header);
        ctr(this.rtpKey, rtpIv(this.rtpSalt, ssrc, roc, sequence), body.subarray(header)).copy(output, header);

        return output;
    }

    encryptRtcp(packet) {
        const ssrc = packet.readUInt32BE(4);
        const index = (this.rtcpIndex = (this.rtcpIndex + 1) & 0x7fffffff);
        const output = Buffer.allocUnsafe(packet.length + 4 + TAG_LENGTH);

        packet.copy(output, 0, 0, 8);
        ctr(this.rtcpKey, rtcpIv(this.rtcpSalt, ssrc, index), packet.subarray(8)).copy(output, 8);
        output.writeUInt32BE((0x80000000 | index) >>> 0, packet.length);
        tag(this.rtcpAuth, output.subarray(0, packet.length + 4)).copy(output, packet.length + 4);

        return output;
    }

    decryptRtcp(packet) {
        if (packet.length < 8 + 4 + TAG_LENGTH) {
            return null;
        }

        const signed = packet.subarray(0, packet.length - TAG_LENGTH);

        if (!equal(tag(this.rtcpAuth, signed), packet.subarray(signed.length))) {
            return null;
        }

        const word = signed.readUInt32BE(signed.length - 4);
        const encrypted = (word & 0x80000000) !== 0;
        const index = word & 0x7fffffff;
        const body = signed.subarray(0, signed.length - 4);

        if (!encrypted) {
            return Buffer.from(body);
        }

        const output = Buffer.allocUnsafe(body.length);

        body.copy(output, 0, 0, 8);
        ctr(this.rtcpKey, rtcpIv(this.rtcpSalt, body.readUInt32BE(4), index), body.subarray(8)).copy(output, 8);

        return output;
    }
}

/** A função de derivação da RFC 3711 (4.3.3) com `kdr = 0`: o rótulo cai no byte 7 do sal. */
const derive = (key, salt, label, length) => {
    const iv = Buffer.alloc(16);

    salt.copy(iv, 0);
    iv[7] ^= label;

    return ctr(key, iv, Buffer.alloc(length));
};

const ctr = (key, iv, data) => createCipheriv('aes-128-ctr', key, iv).update(data);

const tag = (key, ...parts) => {
    const hmac = createHmac('sha1', key);

    for (const part of parts) {
        hmac.update(part);
    }

    return hmac.digest().subarray(0, TAG_LENGTH);
};

const equal = (left, right) => left.length === right.length && timingSafeEqual(left, right);

const rtpIv = (salt, ssrc, roc, sequence) => {
    const iv = Buffer.alloc(16);

    salt.copy(iv, 0);
    iv.writeUInt32BE((iv.readUInt32BE(4) ^ ssrc) >>> 0, 4);
    iv.writeUInt32BE((iv.readUInt32BE(8) ^ roc) >>> 0, 8);
    iv.writeUInt16BE(iv.readUInt16BE(12) ^ sequence, 12);

    return iv;
};

const rtcpIv = (salt, ssrc, index) => {
    const iv = Buffer.alloc(16);

    salt.copy(iv, 0);
    iv.writeUInt32BE((iv.readUInt32BE(4) ^ ssrc) >>> 0, 4);
    iv.writeUInt32BE((iv.readUInt32BE(10) ^ index) >>> 0, 10);

    return iv;
};

const headerLength = packet => {
    let length = 12 + (packet[0] & 0x0f) * 4;

    if (packet[0] & 0x10) {
        length += 4 + packet.readUInt16BE(length + 2) * 4;
    }

    return length;
};

/** Apêndice A da RFC 3711: em que volta do contador de 16 bits este pacote está. */
const estimateRoc = (state, sequence) => {
    if (state.highest < 0x8000) {
        return sequence - state.highest > 0x8000 ? (state.roc - 1) >>> 0 : state.roc;
    }

    return state.highest - 0x8000 > sequence ? (state.roc + 1) >>> 0 : state.roc;
};
