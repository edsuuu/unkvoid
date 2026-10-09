// Minimal SRTP/SRTCP AES_CM_128_HMAC_SHA1_80 (RFC 3711), enough to talk to a mediasoup
// PlainTransport with enableSrtp. ROC is tracked naively (tests never wrap 65535 packets
// from a fixed start, except where they deliberately restart a stream).
import { createCipheriv, createHmac, timingSafeEqual } from 'node:crypto';

const prf = (masterKey, masterSalt, label, length) => {
    const x = Buffer.from(masterSalt);
    x[7] ^= label;
    const iv = Buffer.concat([x, Buffer.alloc(2)]);
    return createCipheriv('aes-128-ctr', masterKey, iv).update(Buffer.alloc(length));
};

export class SrtpContext {
    constructor(keyBase64) {
        const raw = Buffer.from(keyBase64, 'base64');
        if (raw.length !== 30) throw new Error(`expected 30-byte key||salt, got ${raw.length}`);
        const key = raw.subarray(0, 16);
        const salt = raw.subarray(16, 30);
        this.rtpKey = prf(key, salt, 0, 16);
        this.rtpAuth = prf(key, salt, 1, 20);
        this.rtpSalt = prf(key, salt, 2, 14);
        this.rtcpKey = prf(key, salt, 3, 16);
        this.rtcpAuth = prf(key, salt, 4, 20);
        this.rtcpSalt = prf(key, salt, 5, 14);
        this.rtcpIndex = 1;
    }

    static iv(salt, ssrc, index) {
        const iv = Buffer.concat([salt, Buffer.alloc(2)]);
        const s = Buffer.alloc(4);
        s.writeUInt32BE(ssrc >>> 0);
        for (let i = 0; i < 4; i++) iv[4 + i] ^= s[i];
        const idx = Buffer.alloc(8);
        idx.writeBigUInt64BE(BigInt(index));
        for (let i = 0; i < 6; i++) iv[8 + i] ^= idx[2 + i];
        return iv;
    }

    static headerLength(packet) {
        const cc = packet[0] & 0x0f;
        let length = 12 + 4 * cc;
        if (packet[0] & 0x10) {
            length += 4 + 4 * packet.readUInt16BE(length + 2);
        }
        return length;
    }

    encryptRtp(packet, roc = 0) {
        const h = SrtpContext.headerLength(packet);
        const seq = packet.readUInt16BE(2);
        const ssrc = packet.readUInt32BE(8);
        const iv = SrtpContext.iv(this.rtpSalt, ssrc, roc * 65536 + seq);
        const enc = createCipheriv('aes-128-ctr', this.rtpKey, iv).update(packet.subarray(h));
        const body = Buffer.concat([packet.subarray(0, h), enc]);
        const rocBuf = Buffer.alloc(4);
        rocBuf.writeUInt32BE(roc);
        const tag = createHmac('sha1', this.rtpAuth).update(body).update(rocBuf).digest().subarray(0, 10);
        return Buffer.concat([body, tag]);
    }

    decryptRtp(packet, roc = 0) {
        if (packet.length < 22) return null;
        const body = packet.subarray(0, packet.length - 10);
        const tag = packet.subarray(packet.length - 10);
        const rocBuf = Buffer.alloc(4);
        rocBuf.writeUInt32BE(roc);
        const expected = createHmac('sha1', this.rtpAuth).update(body).update(rocBuf).digest().subarray(0, 10);
        if (!timingSafeEqual(tag, expected)) return null;
        const h = SrtpContext.headerLength(body);
        const seq = body.readUInt16BE(2);
        const ssrc = body.readUInt32BE(8);
        const iv = SrtpContext.iv(this.rtpSalt, ssrc, roc * 65536 + seq);
        const dec = createCipheriv('aes-128-ctr', this.rtpKey, iv).update(body.subarray(h));
        return Buffer.concat([body.subarray(0, h), dec]);
    }

    encryptRtcp(packet) {
        const index = this.rtcpIndex++;
        const ssrc = packet.readUInt32BE(4);
        const iv = SrtpContext.iv(this.rtcpSalt, ssrc, index);
        const enc = createCipheriv('aes-128-ctr', this.rtcpKey, iv).update(packet.subarray(8));
        const eidx = Buffer.alloc(4);
        eidx.writeUInt32BE((0x80000000 | index) >>> 0);
        const body = Buffer.concat([packet.subarray(0, 8), enc, eidx]);
        const tag = createHmac('sha1', this.rtcpAuth).update(body).digest().subarray(0, 10);
        return Buffer.concat([body, tag]);
    }

    decryptRtcp(packet) {
        if (packet.length < 8 + 4 + 10) return null;
        const body = packet.subarray(0, packet.length - 10);
        const tag = packet.subarray(packet.length - 10);
        const expected = createHmac('sha1', this.rtcpAuth).update(body).digest().subarray(0, 10);
        if (!timingSafeEqual(tag, expected)) return null;
        const eidx = body.readUInt32BE(body.length - 4);
        const index = eidx & 0x7fffffff;
        const encrypted = (eidx & 0x80000000) !== 0;
        const payload = body.subarray(0, body.length - 4);
        if (!encrypted) return Buffer.from(payload);
        const ssrc = payload.readUInt32BE(4);
        const iv = SrtpContext.iv(this.rtcpSalt, ssrc, index);
        const dec = createCipheriv('aes-128-ctr', this.rtcpKey, iv).update(payload.subarray(8));
        return Buffer.concat([payload.subarray(0, 8), dec]);
    }
}

/** Splits a compound RTCP packet into { pt, fmt, senderSsrc, mediaSsrc, raw }. */
export const parseRtcp = (buffer) => {
    const out = [];
    let offset = 0;
    while (offset + 4 <= buffer.length) {
        const fmt = buffer[offset] & 0x1f;
        const pt = buffer[offset + 1];
        const length = (buffer.readUInt16BE(offset + 2) + 1) * 4;
        const raw = buffer.subarray(offset, offset + length);
        out.push({
            pt,
            fmt,
            senderSsrc: raw.length >= 8 ? raw.readUInt32BE(4) : 0,
            mediaSsrc: raw.length >= 12 ? raw.readUInt32BE(8) : 0,
            raw,
        });
        offset += length;
    }
    return out;
};

export const isRtcp = (buffer) => buffer.length >= 2 && buffer[1] >= 192 && buffer[1] <= 223;
