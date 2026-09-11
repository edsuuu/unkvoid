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
            userId: this.peer.userId,
            // Só para desenhar a interface: expulsar passa pelo Laravel, que confere de
            // novo contra o banco — esconder botão não é autorização.
            owner: this.peer.owner,
        };
    }
}
