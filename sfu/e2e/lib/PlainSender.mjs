import { randomInt } from 'node:crypto';
import { createSocket } from 'node:dgram';

import { H264Packetizer } from './H264Packetizer.mjs';
import { Pacer } from './Pacer.mjs';
import {
    AUDIO_CLOCK,
    PAYLOAD_AUDIO,
    PAYLOAD_VIDEO,
    SOURCE_OFFSET,
    VIDEO_CLOCK,
    buildDelaySinceLastReferenceTime,
    buildRtp,
    buildSenderReport,
    isRtcp,
    isVideoSource,
    readFeedback,
    readReferenceTimes,
} from './rtp.mjs';
import { SrtpSession } from './SrtpSession.mjs';

const HISTORY = 1024;
const SERVER_SILENCE_MS = 5000;
const SENDING_MS = 1000;
const REPORT_MS = 1000;
const SAMPLES_PER_PACKET = 960;

/**
 * O `PlainSender` do app: um socket UDP e uma chave SRTP para todas as origens da pessoa,
 * um SSRC por origem, o relatório do remetente uma vez por segundo, o reenvio do pacote já
 * cifrado quando o servidor pede (NACK) e o aviso de quadro-chave pedido (PLI/FIR).
 */
export class PlainSender {
    /**
     * `extendedReports: true` responde o RRTR do mediasoup com o DLRR, que o app de hoje não
     * manda: sem ele o SFU não sabe a ida e volta da subida e repete o pedido de reenvio a cada
     * 100 ms fixos.
     */
    constructor({ host, port, key, serverKey, ssrcBase, keyframeRouting = 'ssrc', pacing = 'sum', extendedReports = false }) {
        this.host = host;
        this.port = port;
        this.ssrcBase = ssrcBase;
        this.keyframeRouting = keyframeRouting;
        this.pacing = pacing;
        this.extendedReports = extendedReports;
        this.referenceTime = null;
        this.videoRates = new Map();
        this.asked = new Set();
        this.askedShared = false;
        this.outgoing = new SrtpSession(key);
        this.incoming = serverKey ? new SrtpSession(serverKey) : null;
        this.streams = new Map();
        this.history = new Map();
        this.packetizers = new Map();
        this.heardAt = null;
        this.sentAt = null;
        this.stats = { packets: 0, bytes: 0, nacked: 0, resent: 0, keyframeRequests: 0, rtcp: 0 };
        this.socket = createSocket('udp4');
        this.pacer = new Pacer(packet => this.write(packet));
        this.closed = false;
    }

    async open() {
        await new Promise((resolve, reject) => {
            this.socket.once('error', reject);
            this.socket.bind(0, '127.0.0.1', () => {
                this.socket.off('error', reject);
                resolve();
            });
        });

        this.socket.setSendBufferSize(4 * 1024 * 1024);
        this.socket.connect(this.port, this.host);
        await new Promise(resolve => this.socket.once('connect', resolve));
        this.socket.on('message', datagram => this.hear(datagram));
        this.socket.on('error', () => {});

        return this;
    }

    ssrcOf(source) {
        return (this.ssrcBase + SOURCE_OFFSET[source]) >>> 0;
    }

    stream(source) {
        let stream = this.streams.get(source);

        if (!stream) {
            stream = {
                ssrc: this.ssrcOf(source),
                sequence: randomInt(0, 65536),
                timestampBase: randomInt(0, 2 ** 32 - 1),
                firstAt: null,
                lastTimestamp: 0,
                packets: 0,
                bytes: 0,
                reportedAt: 0,
                audioPackets: 0,
            };
            this.streams.set(source, stream);
        }

        return stream;
    }

    /** Um quadro codificado vira os pacotes do `H264Payloader`, todos com o mesmo relógio. */
    sendFrame(source, frame, videoBitrate) {
        if (this.closed) {
            return 0;
        }

        const stream = this.stream(source);

        stream.firstAt ??= frame.capturedAt;
        stream.lastTimestamp = (stream.timestampBase + Math.round((frame.capturedAt - stream.firstAt) * (VIDEO_CLOCK / 1000))) >>> 0;

        let packetizer = this.packetizers.get(source);

        if (!packetizer) {
            packetizer = new H264Packetizer();
            this.packetizers.set(source, packetizer);
        }

        const payloads = packetizer.payloads(frame.nals);

        // O ritmo é um só para todas as origens. `pacing: 'native'` reproduz o app de hoje: cada
        // quadro o redefine com a taxa da própria origem (`follow_bitrate` no `sharing.rs`), e
        // a câmera derruba o ritmo da tela para o piso. O padrão soma as taxas de vídeo.
        this.videoRates.set(source, videoBitrate);
        this.pacer.follow(this.pacing === 'native' ? videoBitrate : [...this.videoRates.values()].reduce((total, rate) => total + rate, 0));
        this.sentAt = Date.now();

        payloads.forEach((payload, index) => {
            const packet = this.protect(stream, PAYLOAD_VIDEO, payload, index === payloads.length - 1);

            this.remember(stream.ssrc, (stream.sequence - 1) & 0xffff, packet);
            this.pacer.push(packet);
        });

        this.report(stream);

        return payloads.length;
    }

    /** O Opus sai na hora, sem passar pelo ritmo, e o relógio anda 20 ms por pacote. */
    sendAudio(source, opus) {
        if (this.closed) {
            return;
        }

        const stream = this.stream(source);

        stream.lastTimestamp = (stream.timestampBase + stream.audioPackets * SAMPLES_PER_PACKET) >>> 0;
        stream.audioPackets += 1;
        this.sentAt = Date.now();
        this.write(this.protect(stream, PAYLOAD_AUDIO, opus, false));
        this.report(stream);
    }

    protect(stream, payloadType, payload, marker) {
        const packet = buildRtp({
            payloadType,
            sequence: stream.sequence,
            timestamp: stream.lastTimestamp,
            ssrc: stream.ssrc,
            marker,
            payload,
        });

        stream.sequence = (stream.sequence + 1) & 0xffff;
        stream.packets += 1;
        stream.bytes += payload.length;

        return this.outgoing.encryptRtp(packet);
    }

    remember(ssrc, sequence, packet) {
        let entries = this.history.get(ssrc);

        if (!entries) {
            entries = new Map();
            this.history.set(ssrc, entries);
        }

        entries.set(sequence, { packet, asked: false });

        if (entries.size > HISTORY) {
            entries.delete(entries.keys().next().value);
        }
    }

    report(stream) {
        const now = Date.now();

        if (now - stream.reportedAt < REPORT_MS) {
            return;
        }

        stream.reportedAt = now;

        if (this.extendedReports && this.referenceTime && isVideoSource([...this.streams].find(([, candidate]) => candidate === stream)?.[0])) {
            const delay = Math.round(((now - this.referenceTime.at) / 1000) * 65536);

            this.write(this.outgoing.encryptRtcp(buildDelaySinceLastReferenceTime(stream.ssrc, this.referenceTime.middle, delay)));
        }

        this.write(
            this.outgoing.encryptRtcp(
                buildSenderReport({
                    ssrc: stream.ssrc,
                    timestamp: stream.lastTimestamp,
                    packets: stream.packets,
                    bytes: stream.bytes,
                }),
            ),
        );
    }

    write(packet) {
        if (this.closed) {
            return;
        }

        this.stats.packets += 1;
        this.stats.bytes += packet.length;
        this.socket.send(packet, () => {});
    }

    hear(datagram) {
        if (!this.incoming || !isRtcp(datagram)) {
            return;
        }

        const rtcp = this.incoming.decryptRtcp(datagram);

        if (!rtcp) {
            return;
        }

        this.heardAt = Date.now();
        this.stats.rtcp += 1;

        for (const middle of readReferenceTimes(rtcp)) {
            this.referenceTime = { middle, at: Date.now() };
        }

        const { keyframes, nacks } = readFeedback(rtcp);

        for (const [source, stream] of this.streams) {
            if (keyframes.has(stream.ssrc) && isVideoSource(source)) {
                this.stats.keyframeRequests += 1;
                this.asked.add(source);
            }
        }

        this.askedShared ||= keyframes.size > 0;

        for (const { ssrc, sequence } of nacks) {
            const entry = this.history.get(ssrc)?.get(sequence);

            if (!entry) {
                continue;
            }

            if (!entry.asked) {
                entry.asked = true;
                this.stats.nacked += 1;
            }

            this.stats.resent += 1;
            this.pacer.pushRepair(entry.packet);
        }
    }

    /**
     * Se o servidor pediu quadro-chave desta origem desde a última pergunta.
     *
     * `keyframeRouting: 'shared'` reproduz o app de hoje: o `read_feedback` do `plain.rs`
     * guarda um pedido só para o remetente inteiro, sem olhar o SSRC, e a primeira origem
     * que pergunta leva — o pedido da câmera pode virar quadro-chave da tela.
     */
    takeKeyframe(source) {
        if (this.keyframeRouting === 'shared') {
            const asked = this.askedShared;

            this.askedShared = false;

            return asked;
        }

        return this.asked.delete(source);
    }

    /** O servidor calado com pacote saindo: o caminho morreu (`lost_the_server`). */
    lostTheServer(now = Date.now()) {
        return this.heardAt !== null && this.sentAt !== null && now - this.sentAt < SENDING_MS && now - this.heardAt >= SERVER_SILENCE_MS;
    }

    close() {
        if (this.closed) {
            return;
        }

        this.closed = true;
        this.pacer.stop();
        this.socket.close();
    }
}
