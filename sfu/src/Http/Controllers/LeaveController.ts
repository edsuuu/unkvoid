import type { Request } from '../Requests/Request.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class LeaveController {
    /**
     * Saída de propósito. Sem isto o servidor trataria como queda e a pessoa continuaria
     * fantasma na lista pelos 30 segundos de carência.
     */
    public handle(request: Request): StatusResource {
        const room = request.room();

        room.removePeer(request.peer());
        request.session.room = null;
        request.session.peer = null;

        return new StatusResource('left');
    }
}
