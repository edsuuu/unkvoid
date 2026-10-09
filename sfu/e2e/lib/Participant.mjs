import { randomBytes } from 'node:crypto';

import { MediaLibrary } from './MediaLibrary.mjs';
import { PlainReceiver } from './PlainReceiver.mjs';
import { PlainSender } from './PlainSender.mjs';
import { PAYLOAD_AUDIO, PAYLOAD_VIDEO, SOURCE_OFFSET, isVideoSource } from './rtp.mjs';
import { SignalClient } from './SignalClient.mjs';
import { VideoTrack } from './VideoTrack.mjs';

const CRYPTO_SUITE = 'AES_CM_128_HMAC_SHA1_80';
const PING_EVERY_MS = 5000;
const PING_PATIENCE_MS = 10_000;
const MOST_REFUSALS = 8;
const RECEIVE_SILENCE_MS = 5000;
const REWATCH_SPACING_MS = 10_000;
const AUDIO_FRAME_MS = 20;

const randomSsrcBase = () => (Math.floor(Math.random() * 0x70000000) + 0x10000000) >>> 0;

/**
 * Um app sem tela, no protocolo do núcleo nativo (`core/src/session.rs`, `room.rs`,
 * `sharing.rs`): entra com a identidade de cada vez, prova a sinalização com `ping` de 5 em
 * 5 s, volta pela `resumeKey` quando o socket cai, republica com chave nova quando a sala
 * não o conhece mais e assiste a tudo o que aparece (`consume_all`).
 *
 * O `moved` segue o contrato (`docs/CONTRATO.md`): para o socket, pede o token do destino e
 * entra lá. `legacyMoved: true` imita o app que não conhece o evento e volta para a origem.
 */
export class Participant {
    constructor({
        name,
        url,
        identity,
        keyframeRouting = 'ssrc',
        pacing = 'sum',
        receiverReports = false,
        extendedReports = false,
        gate = 'native',
        watch = true,
        keepStreams = false,
        legacyMoved = false,
        moveIdentity = null,
        relay = (host, port) => ({ host, port }),
    }) {
        this.name = name;
        this.url = url;
        this.identity = identity;
        this.keyframeRouting = keyframeRouting;
        this.pacing = pacing;
        this.receiverReports = receiverReports;
        this.extendedReports = extendedReports;
        this.gate = gate;
        this.watching = watch;
        this.keepStreams = keepStreams;
        this.legacyMoved = legacyMoved;
        this.moveIdentity = moveIdentity;
        this.relay = relay;

        this.client = null;
        this.peerId = null;
        this.resumeKey = null;
        this.room = null;
        this.can = [];
        this.peers = new Map();
        this.state = 'idle';
        this.left = false;
        this.history = [];
        this.queue = Promise.resolve();
        this.joins = { fresh: 0, resumed: 0, refused: 0 };

        this.sendKey = randomBytes(30);
        this.ssrcBase = randomSsrcBase();
        this.sender = null;
        this.wanted = new Map();
        this.producers = new Map();
        this.loops = new Map();
        this.tracks = new Map();

        this.watchKey = randomBytes(30);
        this.receiver = null;
        this.consumers = new Map();
        this.closedByMe = new Set();
        this.receiving = new Map();
        this.retired = [];
        this.arrival = new Map();
        this.rewatchedAt = 0;
        this.rewatchSpacing = REWATCH_SPACING_MS;
    }

    log(event, data = {}) {
        this.history.push({ at: Date.now(), event, data });
    }

    events(name) {
        return this.history.filter(entry => entry.event === name);
    }

    // ---------------------------------------------------------------- sinalização

    async join() {
        this.left = false;
        await this.connect();

        try {
            await this.requestJoin(false);
        } catch (error) {
            // Entrada recusada não pode deixar o socket aberto segurando o processo.
            this.left = true;
            this.client.drop();
            throw error;
        }

        this.state = 'joined';
        this.supervise();
        await this.settle();

        return this;
    }

    async connect() {
        const client = new SignalClient(this.url);

        await client.open();
        client.onEvent((event, data) => this.enqueue(() => this.handle(event, data)));
        client.onClose(code => this.closed(client, code));
        this.client = client;
    }

    async requestJoin(resume) {
        const identity = await this.identity();
        const answer = await this.client.call('join', { ...identity, resumeKey: this.resumeKey, resume });

        this.peerId = answer.peerId;
        this.resumeKey = answer.resumeKey;
        this.can = answer.can ?? [];
        this.room = identity.room ?? tokenRoom(identity.token);
        this.resumed = answer.resumed;
        this.peers = new Map(
            (answer.peers ?? []).map(peer => [
                peer.peerId,
                { ...peer, producers: new Map(peer.producers.map(producer => [producer.producerId, producer])) },
            ]),
        );
        this.joins[answer.resumed ? 'resumed' : 'fresh'] += 1;
        this.log('joined', { resumed: answer.resumed, room: this.room });

        await this.client.call('voiceState', { muted: false, deafened: false }).catch(() => {});

        return answer;
    }

    supervise() {
        clearInterval(this.beat);
        this.beat = setInterval(() => {
            const client = this.client;

            if (!client || client.closed || this.left) {
                return;
            }

            const sentAt = Date.now();

            client
                .call('ping', {}, PING_PATIENCE_MS)
                .then(() => this.log('ping', { ms: Date.now() - sentAt }))
                .catch(error => {
                    // Resposta de erro prova que o socket vive; só o silêncio derruba.
                    if (error.status !== -1 || client !== this.client) {
                        return;
                    }

                    this.log('signalingMuted');
                    client.drop();
                    this.lost(client);
                });
        }, PING_EVERY_MS);

        clearInterval(this.watchdog);
        this.watchdog = setInterval(() => this.enqueue(() => this.inspect()), 1000);
    }

    closed(client, code) {
        if (client !== this.client) {
            return;
        }

        this.log('closed', { code });

        if (this.left) {
            return;
        }

        this.lost(client);
    }

    lost(client) {
        if (client !== this.client || this.reconnecting) {
            return;
        }

        this.state = 'lost';
        this.reconnecting = this.reconnect().finally(() => (this.reconnecting = null));
    }

    /** O `reconnect` do `session.rs`: espera sorteada, volta com `resume`, senão entra de novo. */
    async reconnect() {
        let attempt = 0;
        let refused = 0;

        while (refused < MOST_REFUSALS && !this.left) {
            const ceiling = Math.min(1000 * 2 ** Math.min(attempt, 16), 10_000);

            attempt += 1;
            await sleep(ceiling / 2 + Math.random() * (ceiling / 2));

            if (this.left) {
                return;
            }

            try {
                await this.connect();
            } catch {
                continue;
            }

            try {
                await this.requestJoin(true);
            } catch {
                try {
                    await this.requestJoin(false);
                } catch (error) {
                    refused += 1;
                    this.joins.refused += 1;
                    this.log('refused', { message: error.message });
                    this.client.drop();
                    continue;
                }
            }

            this.state = 'joined';
            this.log('rejoined', { resumed: this.resumed });
            this.enqueue(() => this.afterRejoin());

            return;
        }

        this.state = 'gone';
        this.log('gone');
    }

    async afterRejoin() {
        if (this.resumed) {
            this.rewatch();
        } else {
            await this.republish();
        }

        await this.settle();
    }

    enqueue(work) {
        this.queue = this.queue.then(work).catch(error => this.log('error', { message: error.message }));

        return this.queue;
    }

    async handle(event, data) {
        this.log(event, data);

        switch (event) {
            case 'peerJoined':
                this.peers.set(data.peerId, { ...data, producers: new Map() });
                break;
            case 'peerLeft':
                this.peers.delete(data.peerId);
                break;
            case 'newProducer': {
                const peer = this.peers.get(data.peerId);

                peer?.producers.set(data.producerId, { producerId: data.producerId, kind: data.kind, source: data.source, paused: false });
                await this.consumeAll();
                break;
            }
            case 'producerClosed':
                for (const peer of this.peers.values()) {
                    peer.producers.delete(data.producerId);
                }

                this.stopWatching(data.producerId);
                break;
            case 'consumerClosed':
                this.stopWatching(data.producerId);
                break;
            case 'producerReceiving':
                this.receiving.set(data.producerId, data.receiving);
                break;
            case 'producerDead':
                await this.died(data);
                break;
            case 'moved':
                await this.moved(data);
                break;
            case 'kicked':
            case 'replaced':
                this.left = true;
                this.state = event;
                this.stopEverything();
                break;
            default:
        }
    }

    // ---------------------------------------------------------------- transmitir

    /**
     * `source` é `screen`, `camera`, `mic` ou `screenAudio`. Vídeo leva `{ width, height,
     * fps, bitrate }`. Abre o producer no servidor e só depois liga o laço que manda.
     */
    async publish(source, options = {}) {
        this.wanted.set(source, options);
        await this.open(source, options);
    }

    async open(source, options) {
        const video = isVideoSource(source);
        const answer = await this.client.call('producePlain', {
            kind: video ? 'video' : 'audio',
            source,
            rtpParameters: rtpParameters(source, this.ssrcBase),
            srtpParameters: { cryptoSuite: CRYPTO_SUITE, keyBase64: this.sendKey.toString('base64') },
        });

        this.producers.set(source, answer.producerId);
        this.log('produced', { source, producerId: answer.producerId });

        if (!this.sender || this.sender.serverPort !== answer.port || this.sender.key !== this.sendKey) {
            this.sender?.close();

            const target = await this.relay(answer.ip, answer.port);

            this.sender = await new PlainSender({
                host: target.host,
                port: target.port,
                key: this.sendKey,
                serverKey: Buffer.from(answer.srtpParameters.keyBase64, 'base64'),
                ssrcBase: this.ssrcBase,
                keyframeRouting: this.keyframeRouting,
                pacing: this.pacing,
                extendedReports: this.extendedReports,
            }).open();
            this.sender.serverPort = answer.port;
            this.sender.key = this.sendKey;
        }

        this.startLoop(source, options);

        return answer.producerId;
    }

    startLoop(source, options) {
        this.stopLoop(source);

        const startedAt = Date.now();
        let sent = 0;

        if (isVideoSource(source)) {
            const track = this.tracks.get(source) ?? new VideoTrack({ ...options, gate: this.gate });

            if (this.tracks.has(source)) {
                track.forceKeyframe = true;
            }

            this.tracks.set(source, track);

            const timer = setInterval(() => {
                const due = Math.floor(((Date.now() - startedAt) * track.fps) / 1000) + 1 - sent;

                for (let index = 0; index < Math.min(due, 2); index += 1) {
                    if (this.sender.takeKeyframe(source)) {
                        track.requestKeyframe();
                    }

                    this.sender.sendFrame(source, track.next(), track.bitrate);
                    sent += 1;
                }

                sent = Math.max(sent, Math.floor(((Date.now() - startedAt) * track.fps) / 1000) - 1);
            }, Math.max(1, Math.floor(1000 / options.fps / 2)));

            this.loops.set(source, timer);

            return;
        }

        const packets = MediaLibrary.opusPackets();
        let position = 0;
        const timer = setInterval(() => {
            const due = Math.floor((Date.now() - startedAt) / AUDIO_FRAME_MS) + 1 - sent;

            for (let index = 0; index < Math.min(due, 3); index += 1) {
                this.sender.sendAudio(source, packets[position]);
                position = (position + 1) % packets.length;
                sent += 1;
            }

            sent = Math.max(sent, Math.floor((Date.now() - startedAt) / AUDIO_FRAME_MS) - 1);
        }, 10);

        this.loops.set(source, timer);
    }

    stopLoop(source) {
        clearInterval(this.loops.get(source));
        this.loops.delete(source);
    }

    /** Trocar a resolução no meio (o app refaz o encoder): o próximo quadro é IDR do tamanho novo. */
    resize(source, width, height) {
        this.tracks.get(source)?.resize(width, height);
        this.wanted.set(source, { ...this.wanted.get(source), width, height });
    }

    async unpublish(source) {
        this.wanted.delete(source);
        this.stopLoop(source);
        this.tracks.delete(source);

        const producerId = this.producers.get(source);

        this.producers.delete(source);

        if (producerId) {
            await this.client.call('closeProducer', { producerId }).catch(() => {});
        }

        if (this.producers.size === 0) {
            this.sender?.close();
            this.sender = null;
            this.sendKey = randomBytes(30);
            this.ssrcBase = randomSsrcBase();
        }
    }

    /** O `resend` do `room.rs`: fecha o que sobrou, chave e SSRC novos, e sobe tudo de novo. */
    async republish() {
        this.stopWatchingAll();

        const wanted = [...this.wanted];
        const old = [...this.producers.values()];

        for (const source of this.loops.keys()) {
            this.stopLoop(source);
        }

        this.producers.clear();

        for (const producerId of old) {
            await this.client.call('closeProducer', { producerId }).catch(() => {});
        }

        this.sender?.close();
        this.sender = null;
        this.sendKey = randomBytes(30);
        this.ssrcBase = randomSsrcBase();
        this.log('republish', { sources: wanted.map(([source]) => source) });

        for (const [source, options] of wanted) {
            await this.open(source, options).catch(error => this.log('republishFailed', { source, message: error.message }));
        }
    }

    async died(data) {
        const mine = [...this.producers.entries()].find(([, producerId]) => producerId === data.producerId);

        if (!mine) {
            return;
        }

        const [source] = mine;

        this.wanted.delete(source);
        this.stopLoop(source);
        this.producers.delete(source);
    }

    // ---------------------------------------------------------------- assistir

    async settle() {
        await this.consumeAll();
    }

    async consumeAll() {
        if (!this.watching || this.state !== 'joined') {
            return;
        }

        for (const peer of this.peers.values()) {
            for (const producer of peer.producers.values()) {
                if (this.consumers.has(producer.producerId) || this.closedByMe.has(producer.producerId)) {
                    continue;
                }

                await this.consume(peer, producer).catch(error => this.log('consumeFailed', { producerId: producer.producerId, message: error.message }));
            }
        }
    }

    async consume(peer, producer) {
        const answer = await this.client.call('consumePlain', {
            producerId: producer.producerId,
            srtpParameters: { cryptoSuite: CRYPTO_SUITE, keyBase64: this.watchKey.toString('base64') },
        });

        if (!this.receiver || this.receiver.serverPort !== answer.port || this.receiver.key !== this.watchKey) {
            this.receiver?.close();

            const target = await this.relay(answer.ip, answer.port);

            this.receiver = await new PlainReceiver({
                host: target.host,
                port: target.port,
                key: this.watchKey,
                serverKey: Buffer.from(answer.srtpParameters.keyBase64, 'base64'),
                keepStreams: this.keepStreams,
                receiverReports: this.receiverReports,
            }).open();
            this.receiver.serverPort = answer.port;
            this.receiver.key = this.watchKey;
        }

        this.receiver.route({
            producerId: producer.producerId,
            kind: answer.kind,
            ssrc: answer.ssrc,
            payloadType: answer.payloadType,
            rtx: answer.rtx,
            label: `${this.name}<-${peer.name}:${answer.source}`,
        });
        this.receiving.set(producer.producerId, answer.receiving);
        this.receiver.routes.get(producer.producerId).watch.reset();

        await this.client.call('resumeConsumer', { consumerId: answer.consumerId });
        this.consumers.set(producer.producerId, { consumerId: answer.consumerId, source: answer.source, peerId: peer.peerId, name: peer.name });
        this.log('consumed', { producerId: producer.producerId, source: answer.source, from: peer.name });
    }

    stopWatching(producerId) {
        const route = this.receiver?.unroute(producerId);

        if (route) {
            this.retired.push({ producerId, summary: route.watch.summary() });
        }

        this.consumers.delete(producerId);
        this.receiving.delete(producerId);
        this.arrival.delete(producerId);
    }

    stopWatchingAll() {
        for (const producerId of [...this.consumers.keys()]) {
            this.stopWatching(producerId);
        }

        this.receiver?.close();
        this.receiver = null;
    }

    /** O `rewatch` do `room.rs`: chave de chegada nova, e o `settle` assiste tudo de novo. */
    rewatch() {
        this.stopWatchingAll();
        this.watchKey = randomBytes(30);
        this.log('rewatch');
    }

    async closeWatched(producerId) {
        const consumer = this.consumers.get(producerId);

        this.closedByMe.add(producerId);
        this.stopWatching(producerId);

        if (consumer) {
            await this.client.call('closeConsumer', { consumerId: consumer.consumerId }).catch(() => {});
        }
    }

    /** O vigia de segundo em segundo: remetente sem RTCP (`lost_the_server`) e tela parada (`ArrivalWatch`). */
    async inspect() {
        if (this.state !== 'joined') {
            return;
        }

        if (this.sender?.lostTheServer()) {
            this.log('lostTheServer');
            await this.republish();
            await this.settle();

            return;
        }

        const now = Date.now();
        let dead = false;

        for (const [producerId, consumer] of this.consumers) {
            if (consumer.source !== 'screen') {
                continue;
            }

            const packets = this.receiver?.routes.get(producerId)?.packets ?? 0;
            const seen = this.arrival.get(producerId);

            if (!seen || seen.packets !== packets || this.receiving.get(producerId) !== true) {
                this.arrival.set(producerId, { packets, at: now });
                continue;
            }

            dead ||= now - seen.at >= RECEIVE_SILENCE_MS;
        }

        if (!dead || now - this.rewatchedAt < this.rewatchSpacing) {
            return;
        }

        this.rewatchedAt = now;
        this.log('arrivalDead');
        this.rewatch();
        await this.settle();
    }

    // ---------------------------------------------------------------- mover e sair

    async moved(data) {
        if (this.legacyMoved) {
            return;
        }

        // Como no `kicked`: este socket não volta mais, e o que subia para.
        this.left = true;
        this.state = 'moved';
        this.client.drop();
        this.stopEverything(false);
        this.log('movedTo', { to: data.to, by: data.by });

        if (!this.moveIdentity) {
            return;
        }

        const wanted = [...this.wanted];

        this.identity = () => this.moveIdentity(data.to);
        this.resumeKey = null;
        this.peers = new Map();
        this.sendKey = randomBytes(30);
        this.ssrcBase = randomSsrcBase();
        this.watchKey = randomBytes(30);
        this.wanted = new Map();
        await this.join();

        for (const [source, options] of wanted) {
            await this.publish(source, options);
        }
    }

    watchSummaries() {
        return [...(this.receiver?.routes.values() ?? [])].map(route => ({ producerId: route.producerId, ...route.watch.summary() }));
    }

    watchOf(producerId) {
        return this.receiver?.routes.get(producerId)?.watch ?? null;
    }

    resetWatches() {
        for (const route of this.receiver?.routes.values() ?? []) {
            route.watch.reset();
        }
    }

    stopEverything(closeSocket = true) {
        clearInterval(this.beat);
        clearInterval(this.watchdog);

        for (const source of [...this.loops.keys()]) {
            this.stopLoop(source);
        }

        this.sender?.close();
        this.sender = null;
        this.producers.clear();
        this.stopWatchingAll();

        if (closeSocket) {
            this.client?.close();
        }
    }

    async leave() {
        this.left = true;
        this.state = 'left';
        await this.client?.call('leave', {}).catch(() => {});
        this.stopEverything();
    }

    /** O app fechado de repente: nada de `leave`, só o socket e a mídia caindo. */
    crash() {
        this.left = true;
        this.state = 'crashed';
        this.client?.drop();
        this.stopEverything(false);
    }
}

/** O `rtp_parameters` do `plain.rs`, campo por campo. */
export const rtpParameters = (source, ssrcBase) => {
    const ssrc = (ssrcBase + SOURCE_OFFSET[source]) >>> 0;

    if (!isVideoSource(source)) {
        return {
            codecs: [{
                mimeType: 'audio/opus',
                payloadType: PAYLOAD_AUDIO,
                clockRate: 48000,
                channels: 2,
                parameters: { useinbandfec: 1, usedtx: 1 },
                rtcpFeedback: [],
            }],
            encodings: [{ ssrc }],
        };
    }

    return {
        codecs: [{
            mimeType: 'video/H264',
            payloadType: PAYLOAD_VIDEO,
            clockRate: 90000,
            parameters: { 'packetization-mode': 1, 'level-asymmetry-allowed': 1, 'profile-level-id': '42e01f' },
            rtcpFeedback: [{ type: 'nack' }, { type: 'nack', parameter: 'pli' }, { type: 'ccm', parameter: 'fir' }, { type: 'goog-remb' }],
        }],
        encodings: [{ ssrc }],
    };
};

const tokenRoom = token => {
    try {
        return JSON.parse(Buffer.from(token.split('.')[0], 'base64url').toString()).room;
    } catch {
        return null;
    }
};

export const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
