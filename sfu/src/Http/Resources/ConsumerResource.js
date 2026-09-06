export class ConsumerResource {
    constructor(consumer, owner) {
        this.consumer = consumer;
        this.owner = owner;
    }

    toArray() {
        return {
            consumerId: this.consumer.id,
            producerId: this.consumer.producerId,
            kind: this.consumer.kind,
            rtpParameters: this.consumer.rtpParameters,
            peerId: this.owner.peer.id,
            name: this.owner.peer.name,
            avatar: this.owner.peer.avatar,
            source: this.owner.producer.appData.source,
        };
    }
}
