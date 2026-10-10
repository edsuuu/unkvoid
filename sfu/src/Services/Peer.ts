import type {
    Consumer,
    PlainTransport,
    Producer,
    Transport,
    WebRtcTransport,
} from 'mediasoup/types';
import type { WebSocket } from 'ws';

import type { MediaRouter } from './Room.js';
import { PERMISSION_BY_SOURCE, type SourceName } from '../Enums/Source.js';
import { ForbiddenException, NotFoundException } from '../Exceptions/ApiException.js';

export type ProducerDescription = {
    producerId: string;
    kind: string;
    source: string;
    paused: boolean;
};

export class Peer {
    public socket: WebSocket;

    public orphanedAt: number | null = null;

    public serverMuted = false;

    public muted = false;

    public deafened = false;

    public media: MediaRouter | null = null;

    public routing: Promise<MediaRouter> | null = null;

    /** A fila de cada sentido do RTP puro: um transporte de subida e um de chegada, nunca dois. */
    public readonly plainOpening = new Map<'send' | 'receive', Promise<unknown>>();

    /** Quantos `producePlain` ainda esperam o worker: o transporte de subida não fecha embaixo deles. */
    public producingPlain = 0;

    private closed = false;

    public readonly transports = new Map<string, WebRtcTransport>();

    public readonly plainTransports = new Map<string, PlainTransport>();

    public readonly producers = new Map<string, Producer>();

    public readonly consumers = new Map<string, Consumer>();

    public constructor(
        public readonly id: string,
        public readonly name: string,
        socket: WebSocket,

        public readonly resumeKey: string,

        public readonly userId: string,

        public can: readonly string[],
        public readonly ip: string,
    ) {
        this.socket = socket;
    }

    public attachSocket(socket: WebSocket): void {
        this.socket = socket;
        this.orphanedAt = null;
    }

    public isOrphaned(): boolean {
        return this.orphanedAt !== null;
    }

    public addTransport(transport: WebRtcTransport): void {
        this.admit(transport);
        this.transports.set(transport.id, transport);
    }

    public addPlainTransport(transport: PlainTransport): void {
        this.admit(transport);
        this.plainTransports.set(transport.id, transport);
        transport.observer.once('close', () => {
            if (this.plainTransports.get(transport.id) === transport) {
                this.plainTransports.delete(transport.id);
            }
        });
    }

    /**
     * O transporte nasce depois de uma ida ao worker, e a pessoa pode ter saído nesse
     * meio-tempo. Guardado num peer fechado, ele ficaria aberto no router até a sala acabar.
     */
    private admit(transport: WebRtcTransport | PlainTransport): void {
        if (this.closed) {
            transport.close();
            throw new NotFoundException('this participant already left the room');
        }
    }

    /**
     * O producer e o consumer também nascem depois de uma ida ao worker, e nesse meio-tempo o
     * transporte pode ter fechado (chave nova, a pessoa saiu) sem levar junto o que ainda não
     * existia: guardado e anunciado assim, ele nunca receberia o `transportclose` que o tira
     * da sala.
     */
    public assertStillOpen(transport: Transport, media: Producer | Consumer): void {
        if (this.closed || transport.closed) {
            media.close();
            throw new NotFoundException('the transport closed before the media was ready');
        }
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

    public getProducer(producerId: string): Producer {
        const producer = this.producers.get(producerId);

        if (!producer) {
            throw new NotFoundException(
                `producer ${producerId} does not exist for this participant`,
            );
        }

        return producer;
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

    public allows(source: SourceName): boolean {
        return this.can.includes(PERMISSION_BY_SOURCE[source]);
    }

    /**
     * O mudo do servidor não recusa o mic: ele nasce pausado (`ProducerController`), e o
     * desmutar o retoma sem a pessoa clicar em nada. Só `resumeProducer` é recusado.
     */
    public assertCanProduce(source: SourceName): void {
        if (!this.allows(source)) {
            throw new ForbiddenException(`this participant cannot produce ${source}`);
        }
    }

    public assertNotServerMuted(source: string): void {
        if (this.serverMuted && source === 'mic') {
            throw new ForbiddenException('this participant was muted by the server');
        }
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
            paused: producer.paused,
        }));
    }

    public sources(): string[] {
        return [...new Set(this.describeProducers().map((producer) => producer.source))];
    }

    public closeProducers(): void {
        for (const producer of this.producers.values()) {
            producer.close();
        }

        this.closePlainTransports();
        this.producers.clear();
    }

    public closePlainTransports(): void {
        for (const transport of this.plainTransports.values()) {
            if (transport.appData.receive === true) {
                continue;
            }

            transport.close();
            this.plainTransports.delete(transport.id);
        }
    }

    public send(event: string, data: unknown): void {
        if (this.socket.readyState !== this.socket.OPEN) {
            return;
        }

        this.socket.send(JSON.stringify({ event, data }));
    }

    public close(): void {
        this.closed = true;

        for (const transport of [...this.transports.values(), ...this.plainTransports.values()]) {
            transport.close();
        }

        this.transports.clear();
        this.plainTransports.clear();
        this.producers.clear();
        this.consumers.clear();
    }
}
