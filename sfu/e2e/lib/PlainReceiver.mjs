import { randomInt } from 'node:crypto';
import { createSocket } from 'node:dgram';

import { AudioWatch } from './AudioWatch.mjs';
import { Recovery } from './Recovery.mjs';
import { buildNack, buildPli, buildReceiverReport, buildRtp, isRtcp, parseRtp, readSenderReports, unwrapRtx } from './rtp.mjs';
import { SrtpSession } from './SrtpSession.mjs';
import { VideoWatch } from './VideoWatch.mjs';

const KEEPALIVE_MS = 5000;
const TICK_MS = 10;
const PLI_INTERVAL_MS = 300;
const CHAIN_ASK_MS = 1000;
const REPORT_MS = 1000;

/**
 * O `PlainReceiver` do app: um socket para todas as transmissões que a pessoa assiste,
 * aberto pelo "furo" (um SRTP válido a cada 5 s, para o `comedia` aprender o endereço e a
 * NAT não esquecer), cada fluxo separado pelo SSRC do `consumePlain`, o RTX desembrulhado,
 * e o vídeo passando pela recuperação (NACK e PLI) antes de virar quadro.
 */
export class PlainReceiver {
    /**
     * `receiverReports: true` manda o RR que o app de hoje não manda: com ele o mediasoup mede
     * a ida e volta e volta a reenviar o pacote cujo reenvio se perdeu (sem ele, ignora o
     * pedido repetido dentro de 100 ms).
     */
    constructor({ host, port, key, serverKey, keepStreams = false, receiverReports = false }) {
        this.host = host;
        this.port = port;
        this.keepStreams = keepStreams;
        this.receiverReports = receiverReports;
        this.senderReports = new Map();
        this.reportedAt = 0;
        this.outgoing = new SrtpSession(key);
        this.incoming = new SrtpSession(serverKey);
        this.ssrc = randomInt(1, 2 ** 32 - 1);
        this.sequence = randomInt(0, 65536);
        this.routes = new Map();
        this.retired = new Set();
        this.stats = { datagrams: 0, rejected: 0, nacks: 0, plis: 0, strays: 0, extended: 0, padding: 0 };
        this.socket = createSocket('udp4');
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

        this.socket.setRecvBufferSize(16 * 1024 * 1024);
        this.socket.connect(this.port, this.host);
        await new Promise(resolve => this.socket.once('connect', resolve));
        this.socket.on('message', datagram => this.arrive(datagram));
        this.socket.on('error', () => {});
        this.punch();
        this.keepalive = setInterval(() => this.punch(), KEEPALIVE_MS);
        this.ticker = setInterval(() => this.tick(), TICK_MS);

        return this;
    }

    punch() {
        const packet = buildRtp({
            payloadType: 96,
            sequence: this.sequence,
            timestamp: 0,
            ssrc: this.ssrc,
            marker: false,
            payload: Buffer.alloc(0),
        });

        this.sequence = (this.sequence + 1) & 0xffff;
        this.send(this.outgoing.encryptRtp(packet));
    }

    /** Uma transmissão nova nesta porta: de quem, com que SSRC, e como medir. */
    route({ producerId, kind, ssrc, payloadType, rtx, label }) {
        const video = kind === 'video';

        this.routes.set(producerId, {
            producerId,
            video,
            ssrc,
            payloadType,
            rtx,
            packets: 0,
            recovery: video ? new Recovery() : null,
            watch: video ? new VideoWatch({ label, keepStream: this.keepStreams }) : new AudioWatch({ label }),
            lastPli: 0,
            lastChainAsk: 0,
            keyframeAsked: false,
        });
    }

    unroute(producerId) {
        const route = this.routes.get(producerId);

        if (route) {
            this.routes.delete(producerId);
            this.retired.add(route.ssrc);
        }

        return route;
    }

    arrive(datagram) {
        this.stats.datagrams += 1;

        if (isRtcp(datagram)) {
            this.hearRtcp(datagram);

            return;
        }

        if (datagram.length < 12) {
            return;
        }

        let plain;

        try {
            plain = this.incoming.decryptRtp(datagram);
        } catch {
            plain = null;
        }

        if (!plain) {
            this.stats.rejected += 1;

            return;
        }

        let parsed = parseRtp(plain);

        if (!parsed) {
            return;
        }

        // O que o app nativo não usa e só ocupa a descida: o pacote só de enchimento. A extensão
        // de cabeçalho é contada para o relatório: o mediasoup reescreve a dele em todo pacote.
        this.stats.extended += plain[0] & 0x10 ? 1 : 0;
        this.stats.padding += parsed.payload.length === 0 ? 1 : 0;

        let route = null;
        let packet = plain;

        for (const candidate of this.routes.values()) {
            if (candidate.rtx && candidate.rtx.ssrc === parsed.ssrc) {
                const original = unwrapRtx(plain, parsed, candidate.ssrc, candidate.payloadType);

                if (!original) {
                    return;
                }

                route = candidate;
                packet = original;
                parsed = parseRtp(original);
                break;
            }

            if (candidate.ssrc === parsed.ssrc) {
                route = candidate;
                break;
            }
        }

        if (!route) {
            this.stats.strays += 1;

            return;
        }

        route.packets += 1;

        const now = Date.now();

        if (!route.video) {
            route.watch.push(parsed, now);

            return;
        }

        if (route.lastArrived !== undefined && ((parsed.sequence - route.lastArrived) & 0xffff) > 0x8000) {
            this.stats.reordered = (this.stats.reordered ?? 0) + 1;
        }

        route.lastArrived = parsed.sequence;

        for (const released of route.recovery.arrive(parsed.sequence, packet, now)) {
            route.watch.push(parseRtp(released.packet), released.gap, now);
        }
    }

    hearRtcp(datagram) {
        if (!this.receiverReports) {
            return;
        }

        const rtcp = this.incoming.decryptRtcp(datagram);

        for (const report of rtcp ? readSenderReports(rtcp) : []) {
            this.senderReports.set(report.ssrc, { middle: report.middle, at: Date.now() });
        }
    }

    report(now) {
        const blocks = [];

        for (const route of this.routes.values()) {
            const last = this.senderReports.get(route.ssrc);

            if (last && route.ssrc) {
                blocks.push({ ssrc: route.ssrc, highest: 0, lastSr: last.middle, delaySinceLastSr: Math.round(((now - last.at) / 1000) * 65536) });
            }
        }

        if (blocks.length > 0) {
            this.send(this.outgoing.encryptRtcp(buildReceiverReport(this.ssrc, blocks.slice(0, 31))));
        }
    }

    tick() {
        const now = Date.now();

        if (this.receiverReports && now - this.reportedAt >= REPORT_MS) {
            this.reportedAt = now;
            this.report(now);
        }

        for (const route of this.routes.values()) {
            if (!route.video) {
                continue;
            }

            const due = route.recovery.due(now);

            for (const released of due.released) {
                route.watch.push(parseRtp(released.packet), released.gap, now);
            }

            if (due.nack.length > 0) {
                this.stats.nacks += 1;
                this.send(this.outgoing.encryptRtcp(buildNack(this.ssrc, route.ssrc, due.nack)));
            }

            // A corrente quebrada (quadro furado que a recuperação não viu) pede o quadro-chave
            // de segundo em segundo, como o `KeyframeStall` do `watching.rs`.
            if (route.watch.waitingKeyframe && now - route.lastChainAsk >= CHAIN_ASK_MS) {
                route.lastChainAsk = now;
                route.keyframeAsked = true;
            }

            if ((due.pli || route.keyframeAsked) && now - route.lastPli >= PLI_INTERVAL_MS) {
                this.stats.plis += 1;
                route.lastPli = now;
                route.keyframeAsked = false;
                this.send(this.outgoing.encryptRtcp(buildPli(this.ssrc, route.ssrc)));
            }
        }
    }

    requestKeyframe(producerId) {
        const route = this.routes.get(producerId);

        if (route) {
            route.keyframeAsked = true;
        }
    }

    send(packet) {
        if (!this.closed) {
            this.socket.send(packet, () => {});
        }
    }

    close() {
        if (this.closed) {
            return;
        }

        this.closed = true;
        clearInterval(this.keepalive);
        clearInterval(this.ticker);
        this.socket.close();
    }
}
