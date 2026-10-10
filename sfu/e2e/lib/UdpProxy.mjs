import { createSocket } from 'node:dgram';

/**
 * A rede ruim entre o app e o SFU, sem `tc netem`: cada `relay` é uma porta local que
 * repassa para o SFU, com perda, atraso e variação configuráveis nos dois sentidos, e o
 * corte total (`cut`) que imita o cabo puxado.
 *
 * A variação anda devagar (um passeio aleatório entre 0 e `jitterMs`) e a fila não
 * ultrapassa: como na internet, o atraso varia sem embaralhar os pacotes. Embaralhar é à
 * parte, `reorder` dos pacotes atrasados um tanto a mais.
 *
 * Um socket de saída por cliente: o `comedia` do mediasoup aprende um endereço só, e este
 * fica estável enquanto o relé viver.
 */
export class UdpProxy {
    /**
     * `rateBps` é o teto da subida (cliente → SFU), como o upload de uma casa: o pacote espera a
     * vez numa fila que anda nessa taxa, e o que acharia mais de `bufferMs` de fila na frente é
     * descartado (o roteador cheio). A descida não tem teto.
     */
    constructor({ loss = 0, delayMs = 0, jitterMs = 0, reorder = 0, rateBps = 0, bufferMs = 200 } = {}) {
        this.loss = loss;
        this.delayMs = delayMs;
        this.jitterMs = jitterMs;
        this.reorder = reorder;
        this.rateBps = rateBps;
        this.bufferMs = bufferMs;
        this.down = false;
        this.closed = false;
        this.relays = [];
        this.stats = { forwarded: 0, dropped: 0, queueDropped: 0, upBytes: 0, maxQueueMs: 0, queuedOver100Ms: 0, queuedOver50Ms: 0, queueSamples: [] };
    }

    configure({ loss = this.loss, delayMs = this.delayMs, jitterMs = this.jitterMs, reorder = this.reorder }) {
        this.loss = loss;
        this.delayMs = delayMs;
        this.jitterMs = jitterMs;
        this.reorder = reorder;
    }

    cut() {
        this.down = true;
    }

    restore() {
        this.down = false;
    }

    /** Uma porta local que leva a `host:port`: é o `relay` que o `Participant` recebe. */
    async relay(host, port) {
        if (this.closed) {
            throw new Error('the UDP proxy is closed');
        }

        const front = createSocket('udp4');
        const back = createSocket('udp4');
        let client = null;

        // O relé nunca segura o processo do teste: quem decide quando acabar são as pessoas.
        front.unref();
        back.unref();

        front.bind(0, '127.0.0.1');
        back.connect(port, host);
        front.on('error', () => {});
        back.on('error', () => {});

        const up = { jitter: 0, last: 0, limited: true, busyUntil: 0 };
        const down = { jitter: 0, last: 0 };

        front.on('message', (datagram, from) => {
            client = from;
            this.forward(up, datagram, packet => back.send(packet, () => {}));
        });

        back.on('message', datagram => {
            if (client) {
                this.forward(down, datagram, packet => front.send(packet, client.port, client.address, () => {}));
            }
        });

        this.relays.push(front, back);

        await Promise.all([new Promise(resolve => front.once('listening', resolve)), new Promise(resolve => back.once('connect', resolve))]);

        return { host: '127.0.0.1', port: front.address().port };
    }

    forward(lane, datagram, send) {
        if (this.down || (this.loss > 0 && Math.random() < this.loss)) {
            this.stats.dropped += 1;

            return;
        }

        const now = performance.now();
        let departAt = now;

        if (lane.limited && this.rateBps > 0) {
            lane.busyUntil = Math.max(lane.busyUntil, now);

            const waitMs = lane.busyUntil - now;

            if (waitMs > this.bufferMs) {
                this.stats.dropped += 1;
                this.stats.queueDropped += 1;
                this.stats.firstDropAt ??= now;

                return;
            }

            lane.busyUntil += ((datagram.length + 28) * 8 * 1000) / this.rateBps;
            departAt = lane.busyUntil;
            this.stats.maxQueueMs = Math.max(this.stats.maxQueueMs, Math.round(waitMs));
            this.stats.queuedOver50Ms += waitMs > 50 ? 1 : 0;
            this.stats.queuedOver100Ms += waitMs > 100 ? 1 : 0;
            this.stats.upBytes += datagram.length;
            this.stats.queueSamples.push([Math.round(now), Math.round(waitMs)]);

            if (this.stats.queueSamples.length > 200_000) {
                this.stats.queueSamples.shift();
            }
        }

        this.stats.forwarded += 1;

        // O relé pode ter fechado enquanto o pacote esperava o atraso.
        const deliver = () => {
            try {
                send(datagram);
            } catch {
                this.stats.dropped += 1;
            }
        };

        if (this.delayMs <= 0 && this.jitterMs <= 0 && this.reorder <= 0 && departAt === now) {
            deliver();

            return;
        }

        lane.jitter = Math.min(Math.max(lane.jitter + (Math.random() - 0.5) * this.jitterMs * 0.1, 0), this.jitterMs);

        if (this.reorder > 0 && Math.random() < this.reorder) {
            setTimeout(deliver, departAt - now + this.delayMs + lane.jitter + 5 + Math.random() * 20);

            return;
        }

        // Uma fila por sentido e um relógio só: o `setTimeout` arredonda o milissegundo, e um
        // relógio por pacote trocava a ordem de quem saía junto.
        lane.queue ??= [];
        lane.queue.push({ at: Math.max(departAt + this.delayMs + lane.jitter, lane.last), deliver });
        lane.last = lane.queue.at(-1).at;
        this.schedule(lane);
    }

    schedule(lane) {
        if (lane.timer || lane.queue.length === 0) {
            return;
        }

        lane.timer = setTimeout(() => {
            lane.timer = null;

            const now = performance.now();

            while (lane.queue.length > 0 && lane.queue[0].at <= now + 0.5) {
                lane.queue.shift().deliver();
            }

            this.schedule(lane);
        }, Math.max(0, lane.queue[0].at - performance.now()));
    }

    close() {
        this.closed = true;

        for (const socket of this.relays) {
            try {
                socket.close();
            } catch {
                // já fechado
            }
        }

        this.relays = [];
    }
}
