import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { RoomRegistry } from '../../Services/RoomRegistry.js';
import { Webhook } from '../../Services/Webhook.js';
import type { JoinRequest } from '../Request/JoinRequest.js';

export class JoinController {
    public constructor(private readonly registry: RoomRegistry) {}

    public async handle(request: JoinRequest): Promise<Payload> {
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

        console.log(
            `[INFO] joined room=${room.id} sub=${peer.userId} name=${JSON.stringify(peer.name)} peer=${peer.id} ip=${peer.ip} resumed=${resumed}`,
        );

        if (!resumed) {
            this.registry.replaceAccount(peer);
            room.broadcast(
                'peerJoined',
                { peerId: peer.id, userId: peer.userId, name: peer.name },
                peer.id,
            );
            Webhook.send('joined', room.id, peer);
        }

        return {
            resumed,
            peerId: peer.id,
            name: peer.name,
            resumeKey: peer.resumeKey,
            routerRtpCapabilities: room.router.rtpCapabilities,
            peers: room.describePeers(peer.id, resumed),
            userId: peer.userId,
            can: peer.can,
        };
    }
}
