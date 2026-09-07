import { ValidationException } from '../../Exceptions/ApiException.js';
import type { PresenceRegistry } from '../../Services/PresenceRegistry.js';
import type { TokenVerifier } from '../../Services/TokenVerifier.js';
import type { JoinRequest } from '../Requests/JoinRequest.js';
import { PresenceResource } from '../Resources/PresenceResource.js';

export class PresenceController {
    constructor(private readonly presence: PresenceRegistry, private readonly tokens: TokenVerifier) {}

    watch(request: JoinRequest): PresenceResource {
        const claims = this.tokens.verify(request.token());

        if (! claims.server) {
            throw new ValidationException('this token is not for server presence');
        }

        if (request.session.watching) {
            this.presence.unwatch(request.session.socket, request.session.watching);
        }

        this.presence.watch(request.session.socket, claims.server);
        request.session.watching = claims.server;

        return new PresenceResource(this.presence.snapshot(claims.server));
    }
}
