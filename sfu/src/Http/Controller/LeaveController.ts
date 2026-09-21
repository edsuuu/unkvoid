import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { Request } from '../Request/Request.js';

export class LeaveController {
    public handle(request: Request): Payload {
        const room = request.room();

        room.removePeer(request.peer());
        request.session.room = null;
        request.session.peer = null;

        return { status: 'left' };
    }
}
