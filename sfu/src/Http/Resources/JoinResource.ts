import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import type { Resource } from '../../types.js';

export class JoinResource implements Resource {
    public constructor(
        private readonly peer: Peer,
        private readonly room: Room,
        private readonly resumed: boolean = false,
    ) {}

    public toArray(): Record<string, unknown> {
        return {
            resumed: this.resumed,
            peerId: this.peer.id,
            name: this.peer.name,
            // Vai só para o dono da sessão. É com ela que ele volta depois de uma queda.
            resumeKey: this.peer.resumeKey,
            routerRtpCapabilities: this.room.router.rtpCapabilities,
            peers: this.room.describePeers(this.peer.id),
            locked: this.room.locked,
            // Só para desenhar a interface. Quem mandar `removePeer` sem ser dono é
            // recusado no servidor de qualquer jeito — esconder botão não é autorização.
            owner: this.room.isOwner(this.peer),
        };
    }
}
