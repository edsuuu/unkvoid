import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { RemovePeerRequest } from '../Request/RemovePeerRequest.js';

export class PeerController {
    public remove(request: RemovePeerRequest): Payload {
        const room = request.room();
        const target = request.target(room);

        if (!target.isOrphaned()) {
            throw new ValidationException(
                'para expulsar alguém use o site: aqui só se remove quem já caiu',
            );
        }

        room.removePeer(target);

        return { status: 'removed' };
    }

    public ping(): Payload {
        return {};
    }
}
