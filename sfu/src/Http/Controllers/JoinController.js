import { JoinResource } from '../Resources/JoinResource.js';

export class JoinController {
    constructor(registry, tokens) {
        this.registry = registry;
        this.tokens = tokens;
    }

    async __invoke(request) {
        const claims = this.tokens.verify(request.token());
        const room = await this.registry.findOrCreate(claims.room);

        const peer = room.addPeer(claims.sub, claims.name ?? 'anônimo', request.session.socket, {
            role: claims.role,
            avatar: claims.avatar ?? null,
        });

        request.session.room = room;
        request.session.peer = peer;

        room.broadcast('peerJoined', {
            peerId: peer.id,
            name: peer.name,
            avatar: peer.avatar,
            role: peer.role,
        }, peer.id);

        return new JoinResource(peer, room);
    }
}
