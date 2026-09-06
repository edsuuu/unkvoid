export class TransportResource {
    constructor(transport) {
        this.transport = transport;
    }

    toArray() {
        return {
            transportId: this.transport.id,
            iceParameters: this.transport.iceParameters,
            iceCandidates: this.transport.iceCandidates,
            dtlsParameters: this.transport.dtlsParameters,
        };
    }
}
