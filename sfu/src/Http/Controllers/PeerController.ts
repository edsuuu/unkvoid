import { ValidationException } from '../../Exceptions/ApiException.js';
import type { RemovePeerRequest } from '../Requests/RemovePeerRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class PeerController {
    remove(request: RemovePeerRequest): StatusResource {
        const room = request.room();
        const target = request.target(room);

        if (! target.isOrphaned()) {
            throw new ValidationException('only a stopped participant can be removed');
        }

        room.removePeer(target);

        return new StatusResource('removed');
    }
}
