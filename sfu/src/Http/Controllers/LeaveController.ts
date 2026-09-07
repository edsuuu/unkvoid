import type { Request } from '../Requests/Request.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class LeaveController {
    constructor(private readonly onPeerGone: (roomId: string, peerId: string) => void) {}

    /**
     * Intentional departure. Without this, the server would treat it as a drop and the person would remain
     * a ghost in the list for the 45-second grace period.
     */
    handle(request: Request): StatusResource {
        const room = request.room();
        const peer = request.peer();

        room.onPeerGone = this.onPeerGone;
        room.removePeer(peer);
        request.session.room = null;
        request.session.peer = null;

        return new StatusResource('left');
    }
}
