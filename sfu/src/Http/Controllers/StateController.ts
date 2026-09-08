import type { PresenceRegistry } from '../../Services/PresenceRegistry.js';
import type { StateRequest } from '../Requests/StateRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class StateController {
    constructor(private readonly presence: PresenceRegistry) {}

    /**
     * Espalha "estou mudo" por dois caminhos, porque são duas plateias diferentes.
     *
     * A presença alcança quem está **fora** do canal — a barra lateral de quem só está
     * lendo o chat, e quem acabou de sair da chamada. O broadcast alcança quem está
     * **dentro**, que monta a lista a partir da sala e não da presença.
     *
     * O autor vem da sessão, nunca do corpo: senão qualquer um mutaria qualquer um.
     */
    handle(request: StateRequest): StatusResource {
        const peer = request.peer();
        const room = request.room();
        const muted = request.muted();
        const deafened = request.deafened();

        this.presence.setState(room.id, peer.id, muted, deafened);
        room.broadcast('peerState', { peerId: peer.id, muted, deafened }, peer.id);

        return new StatusResource('ok');
    }
}
