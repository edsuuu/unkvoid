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
            // Quem retoma compara esta lista com a que já tinha para achar o que perdeu na
            // queda. Sem quem está na carência, ela não saberia dizer se a pessoa saiu ou
            // só caiu também.
            peers: this.room.describePeers(this.peer.id, this.resumed),
            userId: this.peer.userId,
            // Só para desenhar a interface: o servidor confere de novo no `produce`.
            can: this.peer.can,
        };
    }
}
