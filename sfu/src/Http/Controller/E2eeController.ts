import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { E2eeCommitRequest } from '../Request/E2eeCommitRequest.js';
import type { E2eeJoinRequest } from '../Request/E2eeJoinRequest.js';
import type { E2eeWelcomeRequest } from '../Request/E2eeWelcomeRequest.js';

/**
 * The SFU is only an MLS delivery and serialization point. Every `payload` here is base64url
 * bytes forwarded as received; `Room` deliberately stores only an epoch and a payload digest.
 */
export class E2eeController {
    public join(request: E2eeJoinRequest): Payload {
        const delivered = request.room().relayE2eeJoin(request.peer(), request.signal());

        return { status: 'forwarded', delivered };
    }

    public commit(request: E2eeCommitRequest): Payload {
        return request.room().acceptE2eeCommit(request.peer(), request.commit());
    }

    public welcome(request: E2eeWelcomeRequest): Payload {
        request.room().relayE2eeWelcome(request.peer(), request.welcome());

        return { status: 'forwarded' };
    }
}
