import { ValidationException } from '../../Exceptions/ApiException.js';
import type { RemovePeerRequest } from '../Requests/RemovePeerRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class PeerController {
    /**
     * Tira alguém da sala.
     *
     * Duas portas para a mesma ação, e é de propósito. Faxina — remover quem já parou de
     * transmitir e ficou órfão — qualquer um faz, porque não tira ninguém de lugar
     * nenhum: a pessoa já foi embora. Expulsar alguém que está ali, ao vivo, é do dono.
     *
     * Expulsar bane a instalação enquanto a sala existir. Sem isso não é expulsão: o
     * código da sala continua conhecido, e a pessoa volta no segundo seguinte.
     */
    public remove(request: RemovePeerRequest): StatusResource {
        const room = request.room();
        const target = request.target(room);

        if (target.isOrphaned()) {
            room.removePeer(target);

            return new StatusResource('removed');
        }

        if (!room.isOwner(request.peer())) {
            throw new ValidationException('só quem criou a sala pode remover alguém');
        }

        if (target === request.peer()) {
            throw new ValidationException('para sair da sala use o botão de sair');
        }

        room.banPeer(target);

        return new StatusResource('kicked');
    }
}
