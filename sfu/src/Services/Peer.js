import { NotFoundException } from '../Exceptions/ApiException.js';
import { canModerate, Role } from '../Enums/Role.js';

export class Peer {
    constructor(id, name, socket, { role = Role.Member, avatar = null } = {}) {
        this.id = id;
        this.name = name;
        this.role = role;
        this.avatar = avatar;
        this.socket = socket;
        this.transports = new Map();
        this.producers = new Map();
        this.consumers = new Map();
    }

    addTransport(transport) {
        this.transports.set(transport.id, transport);
    }

    getTransport(transportId) {
        const transport = this.transports.get(transportId);

        if (!transport) {
            throw new NotFoundException(`transport ${transportId} não existe neste participante`);
        }

        return transport;
    }

    getConsumer(consumerId) {
        const consumer = this.consumers.get(consumerId);

        if (!consumer) {
            throw new NotFoundException(`consumer ${consumerId} não existe neste participante`);
        }

        return consumer;
    }

    addProducer(producer, source) {
        producer.appData.source = source;
        this.producers.set(producer.id, producer);
    }

    describeProducers() {
        return [...this.producers.values()].map(producer => ({
            producerId: producer.id,
            kind: producer.kind,
            source: producer.appData.source,
        }));
    }

    canModerate() {
        return canModerate(this.role);
    }

    closeProducers() {
        for (const producer of this.producers.values()) {
            producer.close();
        }

        this.producers.clear();
    }

    send(event, data) {
        if (this.socket.readyState !== this.socket.OPEN) {
            return;
        }

        this.socket.send(JSON.stringify({ event, data }));
    }

    close() {
        for (const transport of this.transports.values()) {
            transport.close();
        }

        this.transports.clear();
        this.producers.clear();
        this.consumers.clear();
    }
}
