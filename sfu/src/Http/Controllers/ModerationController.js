import { ForbiddenException } from '../../Exceptions/ApiException.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ModerationController {
    stopBroadcast(request) {
        const target = this.authorize(request);

        target.closeProducers();
        target.send('broadcastStopped', { by: request.peer().name });
        request.room().broadcast('peerProducersClosed', { peerId: target.id });

        return new StatusResource('broadcast-stopped');
    }

    kick(request) {
        const target = this.authorize(request);

        target.send('kicked', { by: request.peer().name });
        target.socket.close();

        return new StatusResource('kicked');
    }

    authorize(request) {
        const actor = request.peer();

        if (! actor.canModerate()) {
            throw new ForbiddenException('você não modera esta sala');
        }

        const target = request.room().findPeer(request.targetPeerId());

        if (target.id === actor.id) {
            throw new ForbiddenException('não dá para moderar você mesmo');
        }

        console.log(`[INFO] ${actor.name} moderou ${target.name} na sala ${request.room().id}`);

        return target;
    }
}
