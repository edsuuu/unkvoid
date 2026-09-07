import { ValidationException } from '../../Exceptions/ApiException.js';
import type { PresenceRegistry } from '../../Services/PresenceRegistry.js';
import type { RoomRegistry } from '../../Services/RoomRegistry.js';
import type { TokenVerifier } from '../../Services/TokenVerifier.js';
import type { JoinRequest } from '../Requests/JoinRequest.js';
import { JoinResource } from '../Resources/JoinResource.js';

export class JoinController {
    constructor(
        private readonly registry: RoomRegistry,
        private readonly tokens: TokenVerifier,
        private readonly presence: PresenceRegistry,
    ) {}

    async handle(request: JoinRequest): Promise<JoinResource> {
        // Sem esta guarda, rejoinar no mesmo socket faria a substituição de sessão
        // fechar o próprio socket antes de responder.
        if (request.session.peer) {
            throw new ValidationException('este socket já entrou em uma sala');
        }

        const claims = this.tokens.verify(request.token());
        const room = await this.registry.findOrCreate(claims.room);

        const { peer, resumed } = room.addPeer(claims.sub, claims.name ?? 'anônimo', request.session.socket, {
            role: claims.role,
            avatar: claims.avatar ?? null,
            resume: request.wantsResume(),
        });

        request.session.room = room;
        request.session.peer = peer;

        this.presence.link(room.id, claims.server);
        this.presence.enter(room.id, peer.id, peer.name, peer.avatar);
        this.presence.setReconnecting(room.id, peer.id, false);

        // Retomada não é novidade para a sala: ninguém saiu, a sinalização só voltou.
        if (! resumed) {
            room.broadcast('peerJoined', {
                peerId: peer.id,
                name: peer.name,
                avatar: peer.avatar,
                role: peer.role,
            }, peer.id);
        }

        return new JoinResource(peer, room, resumed);
    }
}
