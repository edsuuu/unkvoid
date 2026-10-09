// Shared harness: boots the real SFU from sfu/dist, drives it over WebSocket and over UDP
// with real SRTP. Only runs `node sfu/dist/server.js` (build first: `pnpm run build`).
import { spawn } from 'node:child_process';
import { createHmac, randomBytes } from 'node:crypto';
import dgram from 'node:dgram';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

import { SrtpContext, isRtcp, parseRtcp } from './srtp.mjs';

export const SFU_DIR = fileURLToPath(new URL('..', import.meta.url));

const require = createRequire(`${SFU_DIR}/package.json`);
const { WebSocket } = require('ws');

export const SECRET = 'audit-secret-with-more-than-32-characters-xx';

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export const log = (...args) => console.log(`[${new Date().toISOString().slice(11, 23)}]`, ...args);

/** Boots one SFU on its own ports. `base` keeps every scenario on distinct ports. */
export const startSfu = async ({ base, workers = 1, plainPorts = 16, peersPerRouter = 10, extraEnv = {} }) => {
    const env = {
        ...process.env,
        SFU_SECRET: SECRET,
        SFU_HOST: '127.0.0.1',
        SFU_PORT: String(base),
        SFU_MEDIA_PORT: String(base + 10000),
        SFU_PLAIN_PORT: String(base + 20000),
        SFU_PLAIN_PORTS: String(plainPorts),
        SFU_WORKERS: String(workers),
        SFU_PEERS_PER_ROUTER: String(peersPerRouter),
        SFU_ANNOUNCED_ADDRESS: '127.0.0.1',
        SFU_LARAVEL_URL: 'http://127.0.0.1:9', // discard port: webhooks just fail
        SFU_CONNECTIONS_PER_MINUTE: '1000',
        ...extraEnv,
    };
    const child = spawn(process.execPath, ['dist/server.js'], { cwd: SFU_DIR, env, stdio: ['ignore', 'pipe', 'pipe'] });
    const lines = [];
    const onData = (chunk) => {
        for (const line of chunk.toString().split('\n').filter(Boolean)) {
            lines.push(line);
            if (process.env.VERBOSE) console.log('   sfu|', line);
        }
    };
    child.stdout.on('data', onData);
    child.stderr.on('data', onData);
    const started = Date.now();
    while (!lines.some((line) => line.includes('SFU em'))) {
        if (child.exitCode !== null) throw new Error(`SFU exited: ${lines.join('\n')}`);
        if (Date.now() - started > 15000) throw new Error(`SFU did not start: ${lines.join('\n')}`);
        await sleep(50);
    }
    return {
        ws: `ws://127.0.0.1:${base}/sfu`,
        http: `http://127.0.0.1:${base}`,
        lines,
        stop: () => new Promise((resolve) => {
            if (child.exitCode !== null) return resolve();
            child.once('exit', resolve);
            child.kill('SIGTERM');
        }),
    };
};

export const token = (claims) => {
    const body = Buffer.from(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 60, ...claims })).toString('base64url');
    return `${body}.${createHmac('sha256', SECRET).update(body).digest('hex')}`;
};

export const signedPost = async (sfu, path, payload) => {
    const body = JSON.stringify(payload);
    const ts = String(Math.floor(Date.now() / 1000));
    const response = await fetch(`${sfu.http}${path}`, {
        method: 'POST',
        body,
        headers: {
            'content-type': 'application/json',
            'x-unkvoid-timestamp': ts,
            'x-unkvoid-signature': createHmac('sha256', SECRET).update(`${ts}\nPOST\n${path}\n${body}`).digest('hex'),
        },
    });
    return { status: response.status, body: await response.json() };
};

/** A signalling client: `call` returns the full envelope `{ok, data, status, error}`. */
export class Client {
    constructor(url, label) {
        this.url = url;
        this.label = label;
        this.events = [];
        this.waiters = [];
        this.pending = new Map();
        this.next = 1;
        this.closeCode = null;
    }

    async open() {
        this.socket = new WebSocket(this.url);
        this.socket.on('message', (raw) => {
            const message = JSON.parse(raw.toString());
            if (message.event) {
                const entry = { ...message, at: Date.now() };
                this.events.push(entry);
                for (const waiter of [...this.waiters]) {
                    if (waiter.match(entry)) {
                        this.waiters.splice(this.waiters.indexOf(waiter), 1);
                        waiter.resolve(entry);
                    }
                }
                return;
            }
            const resolve = this.pending.get(message.id);
            if (resolve) {
                this.pending.delete(message.id);
                resolve(message);
            }
        });
        this.socket.on('close', (code) => (this.closeCode = code));
        await new Promise((resolve, reject) => {
            this.socket.once('open', resolve);
            this.socket.once('error', reject);
        });
        return this;
    }

    /** Fire without awaiting, so two requests can be in flight in the same tick. */
    call(action, data = {}) {
        const id = this.next++;
        return new Promise((resolve) => {
            this.pending.set(id, resolve);
            this.socket.send(JSON.stringify({ id, action, data }));
        });
    }

    async ok(action, data = {}) {
        const answer = await this.call(action, data);
        if (!answer.ok) throw new Error(`${this.label} ${action} failed: ${answer.status} ${answer.error}`);
        return answer.data;
    }

    waitEvent(name, predicate = () => true, timeoutMs = 5000) {
        const match = (entry) => entry.event === name && predicate(entry.data);
        const found = this.events.find(match);
        if (found) return Promise.resolve(found);
        return new Promise((resolve, reject) => {
            const waiter = { match, resolve };
            this.waiters.push(waiter);
            setTimeout(() => {
                const index = this.waiters.indexOf(waiter);
                if (index >= 0) {
                    this.waiters.splice(index, 1);
                    reject(new Error(`${this.label}: no ${name} in ${timeoutMs}ms`));
                }
            }, timeoutMs);
        });
    }

    eventsNamed(name) {
        return this.events.filter((entry) => entry.event === name);
    }

    terminate() {
        this.socket.terminate();
    }

    close() {
        try {
            this.socket.close();
        } catch {}
    }
}

export const joinGuest = async (sfu, room, name, extra = {}) => {
    const client = await new Client(sfu.ws, name).open();
    const joined = await client.ok('join', { room, name, ...extra });
    client.joined = joined;
    return client;
};

export const joinUser = async (sfu, room, userId, name, can = ['speak', 'stream', 'video'], extra = {}, claims = {}) => {
    const client = await new Client(sfu.ws, name).open();
    const answer = await client.call('join', { token: token({ room, sub: userId, name, can, ...claims }), ...extra });
    if (!answer.ok) throw new Error(`${name} join failed: ${answer.status} ${answer.error}`);
    client.joined = answer.data;
    return client;
};

export const newKey = () => randomBytes(30).toString('base64');

export const VIDEO_PT = 96;
export const AUDIO_PT = 111;

/** The exact rtpParameters the native app sends (native/shared/media/src/plain.rs). */
export const videoParams = (ssrc) => ({
    codecs: [{
        mimeType: 'video/H264',
        payloadType: VIDEO_PT,
        clockRate: 90000,
        parameters: { 'packetization-mode': 1, 'level-asymmetry-allowed': 1, 'profile-level-id': '42e01f' },
        rtcpFeedback: [{ type: 'nack' }, { type: 'nack', parameter: 'pli' }, { type: 'ccm', parameter: 'fir' }, { type: 'goog-remb' }],
    }],
    encodings: [{ ssrc }],
});

export const audioParams = (ssrc) => ({
    codecs: [{ mimeType: 'audio/opus', payloadType: AUDIO_PT, clockRate: 48000, channels: 2, parameters: { useinbandfec: 1, usedtx: 1 }, rtcpFeedback: [] }],
    encodings: [{ ssrc }],
});

export const producePlainBody = (source, ssrc, keyBase64) => ({
    kind: source === 'screen' || source === 'camera' ? 'video' : 'audio',
    source,
    srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64 },
    rtpParameters: source === 'screen' || source === 'camera' ? videoParams(ssrc) : audioParams(ssrc),
});

export const rtpPacket = ({ pt, seq, ts, ssrc, marker = false, payload }) => {
    const header = Buffer.alloc(12);
    header[0] = 0x80;
    header[1] = (marker ? 0x80 : 0) | pt;
    header.writeUInt16BE(seq & 0xffff, 2);
    header.writeUInt32BE(ts >>> 0, 4);
    header.writeUInt32BE(ssrc >>> 0, 8);
    return Buffer.concat([header, payload]);
};

// H.264: mediasoup marks a packet as keyframe when it carries an SPS (NAL 7).
export const SPS = Buffer.from([0x67, 0x42, 0xe0, 0x1f, 0xda, 0x01, 0x40, 0x16, 0xec, 0x04, 0x40]);
export const IDR = Buffer.concat([Buffer.from([0x65, 0x88, 0x84]), Buffer.alloc(200, 0x11)]);
export const DELTA = Buffer.concat([Buffer.from([0x41, 0x9a, 0x02]), Buffer.alloc(200, 0x22)]);
export const OPUS = Buffer.from([0xfc, 0xff, 0xfe]);

/**
 * The publisher side of a PlainTransport: SRTP out with our key, SRTCP in with the
 * server key. Keeps every RTCP it gets back (RR, NACK, PLI, FIR).
 */
export class PlainSender {
    constructor(label) {
        this.label = label;
        this.socket = dgram.createSocket('udp4');
        this.rtcp = [];
        this.undecryptable = 0;
        this.streams = new Map();
    }

    async bind() {
        await new Promise((resolve) => this.socket.bind(0, '127.0.0.1', resolve));
        this.socket.on('message', (buffer) => {
            if (!isRtcp(buffer) || !this.incoming) return;
            const plain = this.incoming.decryptRtcp(buffer);
            if (!plain) {
                this.undecryptable += 1;
                return;
            }
            for (const entry of parseRtcp(plain)) {
                this.rtcp.push({ ...entry, at: Date.now() });
                // Like the native encoder: a PLI/FIR forces the next frame to be a keyframe.
                if ((entry.pt === 206 && (entry.fmt === 1 || entry.fmt === 4)) && this.answerPli !== false) {
                    this.keyframeWanted = true;
                }
            }
        });
        return this;
    }

    point({ ip, port, srtpParameters }, ourKey) {
        this.address = ip;
        this.port = port;
        this.outgoing = new SrtpContext(ourKey);
        this.incoming = new SrtpContext(srtpParameters.keyBase64);
    }

    /** Sends one video frame: keyframe if asked for (PLI) or every `gop` frames. */
    frame(ssrc, { gop = 0, skip = false } = {}) {
        this.frames = (this.frames ?? 0) + 1;
        if (this.keyframeWanted || (gop && this.frames % gop === 1)) {
            this.keyframeWanted = false;
            this.sendKeyframe(ssrc);
        } else {
            this.sendDelta(ssrc, { skip });
        }
    }

    send(packet) {
        this.socket.send(this.outgoing.encryptRtp(packet), this.port, this.address);
    }

    stream(ssrc, start = Math.floor(Math.random() * 30000)) {
        if (!this.streams.has(ssrc)) this.streams.set(ssrc, { seq: start, ts: Math.floor(Math.random() * 1e9) });
        return this.streams.get(ssrc);
    }

    sendKeyframe(ssrc) {
        const state = this.stream(ssrc);
        state.ts += 3000;
        this.send(rtpPacket({ pt: VIDEO_PT, seq: state.seq++, ts: state.ts, ssrc, payload: SPS }));
        this.send(rtpPacket({ pt: VIDEO_PT, seq: state.seq++, ts: state.ts, ssrc, marker: true, payload: IDR }));
    }

    sendDelta(ssrc, { skip = false } = {}) {
        const state = this.stream(ssrc);
        state.ts += 3000;
        if (skip) state.seq++;
        this.send(rtpPacket({ pt: VIDEO_PT, seq: state.seq++, ts: state.ts, ssrc, marker: true, payload: DELTA }));
    }

    sendOpus(ssrc) {
        const state = this.stream(ssrc);
        state.ts += 960;
        this.send(rtpPacket({ pt: AUDIO_PT, seq: state.seq++, ts: state.ts, ssrc, payload: OPUS }));
    }

    plis(ssrc) {
        return this.rtcp.filter((entry) => entry.pt === 206 && entry.fmt === 1 && (ssrc === undefined || entry.mediaSsrc === ssrc));
    }

    nacks(ssrc) {
        return this.rtcp.filter((entry) => entry.pt === 205 && entry.fmt === 1 && (ssrc === undefined || entry.mediaSsrc === ssrc));
    }

    close() {
        clearInterval(this.timer);
        try {
            this.socket.close();
        } catch {}
    }
}

/** The watcher side: punches the comedia path, decrypts what the SFU sends. */
export class PlainReceiver {
    constructor(label) {
        this.label = label;
        this.socket = dgram.createSocket('udp4');
        this.rtp = [];
        this.rtcp = [];
        this.undecryptable = 0;
        this.punchSeq = 1;
        this.punchSsrc = 0x5eed0000 + Math.floor(Math.random() * 1000);
    }

    async bind() {
        await new Promise((resolve) => this.socket.bind(0, '127.0.0.1', resolve));
        this.socket.on('message', (buffer) => {
            if (!this.incoming) return;
            if (isRtcp(buffer)) {
                const plain = this.incoming.decryptRtcp(buffer);
                if (plain) for (const entry of parseRtcp(plain)) this.rtcp.push({ ...entry, at: Date.now() });
                return;
            }
            const plain = this.incoming.decryptRtp(buffer);
            if (!plain) {
                this.undecryptable += 1;
                return;
            }
            const h = SrtpContext.headerLength(plain);
            this.rtp.push({
                at: Date.now(),
                pt: plain[1] & 0x7f,
                seq: plain.readUInt16BE(2),
                ssrc: plain.readUInt32BE(8),
                nal: plain[h] & 0x1f,
                payload: plain.subarray(h),
                extension: (plain[0] & 0x10) !== 0,
            });
        });
        return this;
    }

    point({ ip, port, srtpParameters }, ourKey) {
        this.address = ip;
        this.port = port;
        this.outgoing = new SrtpContext(ourKey);
        this.incoming = new SrtpContext(srtpParameters.keyBase64);
    }

    punch() {
        const packet = rtpPacket({ pt: 96, seq: this.punchSeq++, ts: 0, ssrc: this.punchSsrc, payload: Buffer.alloc(4) });
        this.socket.send(this.outgoing.encryptRtp(packet), this.port, this.address);
    }

    sendRtcp(packet) {
        this.socket.send(this.outgoing.encryptRtcp(packet), this.port, this.address);
    }

    /** RTCP PSFB PLI (RFC 4585), what the native receiver sends after a 250 ms hole. */
    pli(mediaSsrc) {
        const packet = Buffer.alloc(12);
        packet[0] = 0x81;
        packet[1] = 206;
        packet.writeUInt16BE(2, 2);
        packet.writeUInt32BE(this.punchSsrc, 4);
        packet.writeUInt32BE(mediaSsrc >>> 0, 8);
        this.sendRtcp(packet);
    }

    /** RTCP RTPFB generic NACK for one sequence number. */
    nack(mediaSsrc, seq) {
        const packet = Buffer.alloc(16);
        packet[0] = 0x81;
        packet[1] = 205;
        packet.writeUInt16BE(3, 2);
        packet.writeUInt32BE(this.punchSsrc, 4);
        packet.writeUInt32BE(mediaSsrc >>> 0, 8);
        packet.writeUInt16BE(seq & 0xffff, 12);
        packet.writeUInt16BE(0, 14);
        this.sendRtcp(packet);
    }

    of(ssrc) {
        return this.rtp.filter((entry) => entry.ssrc === ssrc);
    }

    close() {
        try {
            this.socket.close();
        } catch {}
    }
}

/** Publishes one source the way the native app does and returns the sender + answer. */
export const publish = async (client, source, ssrc, key = newKey(), sender = null) => {
    const answer = await client.ok('producePlain', producePlainBody(source, ssrc, key));
    sender ??= await new PlainSender(`${client.label}/${source}`).bind();
    sender.point(answer, key);
    return { answer, sender, key };
};

/** Watches one producer the way the native app does (consumePlain + punch + resume). */
export const watch = async (client, producerId, receiver = null, key = null) => {
    receiver ??= await new PlainReceiver(`${client.label}/rx`).bind();
    key ??= receiver.key ?? newKey();
    receiver.key = key;
    const answer = await client.ok('consumePlain', { producerId, srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: key } });
    receiver.point(answer, key);
    receiver.punch();
    await sleep(30);
    await client.ok('resumeConsumer', { consumerId: answer.consumerId });
    return { answer, receiver };
};

export const check = (label, condition, detail = '') => {
    console.log(`${condition ? 'PASS' : 'FAIL'}  ${label}${detail ? `  — ${detail}` : ''}`);
    return condition;
};
