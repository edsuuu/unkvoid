import { ValidationException } from '../../Exceptions/ApiException.js';
import type { RoomRegistry } from '../../Services/RoomRegistry.js';
import { Webhook } from '../../Services/Webhook.js';
import type { JoinRequest } from '../Requests/JoinRequest.js';
import { JoinResource } from '../Resources/JoinResource.js';

export class JoinController {
    public constructor(private readonly registry: RoomRegistry) {}

    public async handle(request: JoinRequest): Promise<JoinResource> {
        // Sem esta guarda, entrar de novo no mesmo socket faria a substituição de sessão
        // fechar o próprio socket antes de responder.
        if (request.session.peer) {
            throw new ValidationException('this socket has already joined a room');
        }

        const room = await this.registry.findOrCreate(request.roomCode());

        const { peer, resumed } = room.addPeer(
            request.name(),
            request.session.socket,
            { userId: request.userId(), can: request.can(), ip: request.session.ip },
            { resumeKey: request.resumeKey(), resume: request.wantsResume() },
        );

        request.session.room = room;
        request.session.peer = peer;

        // Retomada não é novidade para a sala: ninguém saiu, a sinalização é que voltou.
        if (!resumed) {
            room.broadcast(
                'peerJoined',
                { peerId: peer.id, userId: peer.userId, name: peer.name },
                peer.id,
            );
            Webhook.send('joined', room.id, peer);
        }

        return new JoinResource(peer, room, resumed);
    }
}
