export class JoinResource {
    constructor(peer, room) {
        this.peer = peer;
        this.room = room;
    }

    toArray() {
        return {
            peerId: this.peer.id,
            name: this.peer.name,
            role: this.peer.role,
            routerRtpCapabilities: this.room.router.rtpCapabilities,
            peers: this.room.describePeers(this.peer.id),
        };
    }
}
