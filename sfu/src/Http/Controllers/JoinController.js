import { ValidationException } from '../../Exceptions/ApiException.js';
import { JoinResource } from '../Resources/JoinResource.js';

export class JoinController {
    constructor(registry, tokens) {
        this.registry = registry;
        this.tokens = tokens;
    }

    async __invoke(request) {
        // Sem esta guarda, rejoinar no mesmo socket faria a substituição de sessão
        // fechar o próprio socket antes de responder.
        if (request.session.peer) {
            throw new ValidationException('este socket já entrou em uma sala');
        }

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
