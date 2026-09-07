import { ForbiddenException } from '../../Exceptions/ApiException.js';
import type { Peer } from '../../Services/Peer.js';
import type { ModerationRequest } from '../Requests/ModerationRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class ModerationController {
    /**
     * Encerra a transmissão sem tirar a pessoa da sala: ela continua na chamada e no
     * chat, só para de publicar. Expulsar do servidor é outra coisa, e mora no Laravel.
     */
    stopBroadcast(request: ModerationRequest): StatusResource {
        const target = this.authorize(request);

        target.closeProducers();
        target.send('broadcastStopped', { by: request.peer().name });
        request.room().broadcast('peerProducersClosed', { peerId: target.id });

        return new StatusResource('broadcast-stopped');
    }

    /**
     * Tira da chamada de voz. A pessoa segue membro do servidor e do chat.
     */
    disconnect(request: ModerationRequest): StatusResource {
        const target = this.authorize(request);

        target.send('disconnected', { by: request.peer().name });
        target.socket.close();

        return new StatusResource('disconnected');
    }

    private authorize(request: ModerationRequest): Peer {
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
