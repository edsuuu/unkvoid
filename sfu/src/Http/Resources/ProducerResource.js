export class ProducerResource {
    constructor(producer) {
        this.producer = producer;
    }

    toArray() {
        return {
            producerId: this.producer.id,
            kind: this.producer.kind,
            source: this.producer.appData.source,
        };
    }
}
