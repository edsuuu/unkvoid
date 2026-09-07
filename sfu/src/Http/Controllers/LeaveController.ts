import type { Request } from '../Requests/Request.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class LeaveController {
    constructor(private readonly onPeerGone: (roomId: string, peerId: string) => void) {}

    /**
     * Saída de propósito. Sem isto o servidor tratava como queda e a pessoa ficava
     * fantasma na lista pelos 45 s de carência.
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
