import { ForbiddenException } from '../../Exceptions/ApiException.js';
import type { Peer } from '../../Services/Peer.js';
import type { ModerationRequest } from '../Requests/ModerationRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ModerationController {
    constructor(private readonly onBroadcastStopped: (roomId: string, peerId: string) => void) {}

    /**
     * Stops the broadcast without removing the person from the room: they remain in the call and
     * chat, but stop publishing. Removing someone from the server is separate and belongs in Laravel.
     */
    stopBroadcast(request: ModerationRequest): StatusResource {
        const target = this.authorize(request);

        target.closeProducers();
        this.onBroadcastStopped(request.room().id, target.id);
        target.send('broadcastStopped', { by: request.peer().name });
        request.room().broadcast('peerProducersClosed', { peerId: target.id });

        return new StatusResource('broadcast-stopped');
    }

    /**
     * Removes the person from the voice call. They remain a server and chat member.
     */
    disconnect(request: ModerationRequest): StatusResource {
        const target = this.authorize(request);

        target.send('disconnected', { by: request.peer().name });
        target.socket.close();

        return new StatusResource('disconnected');
    }

    private authorize(request: ModerationRequest): Peer {
        const actor = request.peer();

        if (! actor.canModerate()) {
            throw new ForbiddenException('you do not moderate this room');
        }

        const target = request.room().findPeer(request.targetPeerId());

        if (target.id === actor.id) {
            throw new ForbiddenException('you cannot moderate yourself');
        }

        console.log(`[INFO] ${actor.name} moderated ${target.name} in room ${request.room().id}`);

        return target;
    }
}
