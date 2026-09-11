import { ValidationException } from '../../Exceptions/ApiException.js';
import type { RemovePeerRequest } from '../Requests/RemovePeerRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class PeerController {
    /**
     * Faxina: tira da lista quem já parou de transmitir e ficou órfão. Qualquer um faz,
     * porque não tira ninguém de lugar nenhum — a pessoa já foi embora.
     *
     * Expulsar alguém ao vivo não passa por aqui. Vem do Laravel, pelo endpoint HTTP
     * assinado, depois de gravar o banimento: sem a segunda metade a pessoa pede outro
     * token e volta no segundo seguinte.
     */
    public remove(request: RemovePeerRequest): StatusResource {
        const room = request.room();
        const target = request.target(room);

        if (!target.isOrphaned()) {
            throw new ValidationException(
                'para expulsar alguém use o site: aqui só se remove quem já caiu',
            );
        }

        room.removePeer(target);

        return new StatusResource('removed');
    }
}
