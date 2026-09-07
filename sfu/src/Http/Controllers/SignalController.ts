import type { SignalRequest } from '../Requests/SignalRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class SignalController {
    /**
     * Forwards the message to the recipient. The sender comes from the session, never the body:
     * otherwise someone could impersonate another person in the room.
     */
    handle(request: SignalRequest): StatusResource {
        const from = request.peer();
        const to = request.room().findPeer(request.to());

        to.send('signal', {
            from: from.id,
            name: from.name,
            kind: request.kind(),
            payload: request.payload(),
        });

        return new StatusResource('entregue');
    }
}
