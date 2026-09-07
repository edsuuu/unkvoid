import type { Consumer, Producer, WebRtcTransport } from 'mediasoup/types';
import type { WebSocket } from 'ws';

import { canModerate, Role, type RoleName } from '../Enums/Role.js';
import { NotFoundException } from '../Exceptions/ApiException.js';
import type { ProducerDescription } from '../types.js';

type PeerOptions = {
    role?: RoleName;
    avatar?: string | null;
};

export class Peer {
    public readonly transports = new Map<string, WebRtcTransport>();

    public readonly producers = new Map<string, Producer>();

    public readonly consumers = new Map<string, Consumer>();

    public readonly role: RoleName;

    public readonly avatar: string | null;

    constructor(
        public readonly id: string,
        public readonly name: string,
        public readonly socket: WebSocket,
        options: PeerOptions = {},
    ) {
        this.role = options.role ?? Role.Member;
        this.avatar = options.avatar ?? null;
    }

    addTransport(transport: WebRtcTransport): void {
        this.transports.set(transport.id, transport);
    }

    getTransport(transportId: string): WebRtcTransport {
        const transport = this.transports.get(transportId);

        if (!transport) {
            throw new NotFoundException(`transport ${transportId} não existe neste participante`);
        }

        return transport;
    }

    getConsumer(consumerId: string): Consumer {
        const consumer = this.consumers.get(consumerId);

        if (!consumer) {
            throw new NotFoundException(`consumer ${consumerId} não existe neste participante`);
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

        this.producers.clear();
    }

    send(event: string, data: unknown): void {
        if (this.socket.readyState !== this.socket.OPEN) {
            return;
        }

        this.socket.send(JSON.stringify({ event, data }));
    }

    close(): void {
        for (const transport of this.transports.values()) {
            transport.close();
        }

        this.transports.clear();
        this.producers.clear();
        this.consumers.clear();
    }
}
