import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import type { Resource } from '../../types.js';

export class JoinResource implements Resource {
    constructor(private readonly peer: Peer, private readonly room: Room) {}

    toArray(): Record<string, unknown> {
        return {
            peerId: this.peer.id,
            name: this.peer.name,
            role: this.peer.role,
            routerRtpCapabilities: this.room.router.rtpCapabilities,
            peers: this.room.describePeers(this.peer.id),
        };
    }
}
