import type { Consumer, PlainTransport, Producer, WebRtcTransport } from 'mediasoup/types';
import type { WebSocket } from 'ws';

import { NotFoundException } from '../Exceptions/ApiException.js';
import type { ProducerDescription } from '../types.js';

export class Peer {
    public socket: WebSocket;

    /** Quando o socket caiu. Nulo enquanto a sinalização está viva. */
    public orphanedAt: number | null = null;

    public readonly transports = new Map<string, WebRtcTransport>();

    /** O ingest de RTP puro do app nativo. Mapa separado: não tem DTLS a conectar. */
    public readonly plainTransports = new Map<string, PlainTransport>();

    public readonly producers = new Map<string, Producer>();

    public readonly consumers = new Map<string, Consumer>();

    public constructor(
        public readonly id: string,
        public readonly name: string,
        socket: WebSocket,
        /** Segredo desta sessão: quem o apresenta de volta é a mesma pessoa, e mais ninguém. */
        public readonly resumeKey: string,
    ) {
        this.socket = socket;
    }

    /**
     * Troca a sinalização sem tocar na mídia: transports, producers e consumers seguem
     * vivos, então a tela de quem assiste não pisca.
     */
    public attachSocket(socket: WebSocket): void {
        this.socket = socket;
        this.orphanedAt = null;
    }

    public isOrphaned(): boolean {
        return this.orphanedAt !== null;
    }

    public addTransport(transport: WebRtcTransport): void {
        this.transports.set(transport.id, transport);
    }

    public addPlainTransport(transport: PlainTransport): void {
        this.plainTransports.set(transport.id, transport);
    }

    public getTransport(transportId: string): WebRtcTransport {
        const transport = this.transports.get(transportId);

        if (!transport) {
            throw new NotFoundException(
                `transport ${transportId} does not exist for this participant`,
            );
        }

        return transport;
    }

    public getConsumer(consumerId: string): Consumer {
        const consumer = this.consumers.get(consumerId);

        if (!consumer) {
            throw new NotFoundException(
                `consumer ${consumerId} does not exist for this participant`,
            );
        }

        return consumer;
    }

    public addProducer(producer: Producer, source: string): void {
        producer.appData.source = source;
        this.producers.set(producer.id, producer);
    }

    public describeProducers(): ProducerDescription[] {
        return [...this.producers.values()].map((producer) => ({
            producerId: producer.id,
            kind: producer.kind,
            source: String(producer.appData.source),
        }));
    }

    public closeProducers(): void {
        for (const producer of this.producers.values()) {
            producer.close();
        }

        this.closePlainTransports();
        this.producers.clear();
    }

    public closePlainTransports(): void {
        // Um plain transport existe só para carregar uma transmissão: deixá-lo aberto
        // seguraria uma porta UDP de uma faixa estreita pelo resto da vida do processo.
        for (const transport of this.plainTransports.values()) {
            transport.close();
        }

        this.plainTransports.clear();
    }

    public send(event: string, data: unknown): void {
        if (this.socket.readyState !== this.socket.OPEN) {
            return;
        }

        this.socket.send(JSON.stringify({ event, data }));
    }

    public close(): void {
        for (const transport of [...this.transports.values(), ...this.plainTransports.values()]) {
            transport.close();
        }

        this.transports.clear();
        this.plainTransports.clear();
        this.producers.clear();
        this.consumers.clear();
    }
}
