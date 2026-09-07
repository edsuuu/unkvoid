import type { Consumer, PlainTransport, Producer, WebRtcTransport } from 'mediasoup/types';
import type { WebSocket } from 'ws';

import { canModerate, Role, type RoleName } from '../Enums/Role.js';
import { NotFoundException } from '../Exceptions/ApiException.js';
import type { ProducerDescription } from '../types.js';

type PeerOptions = {
    role?: RoleName;
    avatar?: string | null;
};

export class Peer {
    public socket: WebSocket;

    /** Time when the socket dropped. Null while signaling is alive. */
    public orphanedAt: number | null = null;

    public readonly transports = new Map<string, WebRtcTransport>();

    /** Plain RTP ingest from the native app. Separate map: it has no DTLS to connect. */
    public readonly plainTransports = new Map<string, PlainTransport>();

    public readonly producers = new Map<string, Producer>();

    public readonly consumers = new Map<string, Consumer>();

    public readonly role: RoleName;

    public readonly avatar: string | null;

    constructor(
        public readonly id: string,
        public readonly name: string,
        socket: WebSocket,
        options: PeerOptions = {},
    ) {
        this.socket = socket;
        this.role = options.role ?? Role.Member;
        this.avatar = options.avatar ?? null;
    }

    /**
     * Switches signaling without touching media: transports, producers, and consumers
     * remain alive, so the viewer’s screen does not flicker.
     */
    attachSocket(socket: WebSocket): void {
        this.socket = socket;
        this.orphanedAt = null;
    }

    isOrphaned(): boolean {
        return this.orphanedAt !== null;
    }

    addTransport(transport: WebRtcTransport): void {
        this.transports.set(transport.id, transport);
    }

    addPlainTransport(transport: PlainTransport): void {
        this.plainTransports.set(transport.id, transport);
    }

    getTransport(transportId: string): WebRtcTransport {
        const transport = this.transports.get(transportId);

        if (!transport) {
            throw new NotFoundException(`transport ${transportId} does not exist for this participant`);
        }

        return transport;
    }

    getConsumer(consumerId: string): Consumer {
        const consumer = this.consumers.get(consumerId);

        if (!consumer) {
            throw new NotFoundException(`consumer ${consumerId} does not exist for this participant`);
        }

        return consumer;
    }

    addProducer(producer: Producer, source: string): void {
        producer.appData.source = source;
        this.producers.set(producer.id, producer);
    }

    describeProducers(): ProducerDescription[] {
        return [...this.producers.values()].map(producer => ({
            producerId: producer.id,
            kind: producer.kind,
            source: String(producer.appData.source),
        }));
    }

    canModerate(): boolean {
        return canModerate(this.role);
    }

    closeProducers(): void {
        for (const producer of this.producers.values()) {
            producer.close();
        }

        // A plain transport exists only to carry one broadcast: leaving it open would
        // hold a UDP port from a small band for the rest of the process's life.
        for (const transport of this.plainTransports.values()) {
            transport.close();
        }

        this.plainTransports.clear();
        this.producers.clear();
    }

    send(event: string, data: unknown): void {
        if (this.socket.readyState !== this.socket.OPEN) {
            return;
        }

        this.socket.send(JSON.stringify({ event, data }));
    }

    close(): void {
        for (const transport of [...this.transports.values(), ...this.plainTransports.values()]) {
            transport.close();
        }

        this.transports.clear();
        this.plainTransports.clear();
        this.producers.clear();
        this.consumers.clear();
    }
}
