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
        // Without this guard, rejoining on the same socket would make session replacement
        // close its own socket before responding.
        if (request.session.peer) {
            throw new ValidationException('this socket has already joined a room');
        }

        const claims = this.tokens.verify(request.token());
        const room = await this.registry.findOrCreate(claims.room);

        const { peer, resumed } = room.addPeer(claims.sub, claims.name ?? 'anonymous', request.session.socket, {
            role: claims.role,
            avatar: claims.avatar ?? null,
            resume: request.wantsResume(),
        });

        request.session.room = room;
        request.session.peer = peer;

        this.presence.link(room.id, claims.server);
        this.presence.enter(room.id, peer.id, peer.name, peer.avatar);
        this.presence.setReconnecting(room.id, peer.id, false);

        // A resume is not new to the room: nobody left; signaling simply returned.
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
