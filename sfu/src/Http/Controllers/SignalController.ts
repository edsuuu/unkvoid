import type { SignalRequest } from '../Requests/SignalRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class SignalController {
    /**
     * Repassa a mensagem ao destinatário. O remetente vem da sessão, nunca do corpo:
     * senão daria para se passar por outra pessoa na sala.
     */
    handle(request: SignalRequest): StatusResource {
        const origem = request.peer();
        const destino = request.room().findPeer(request.to());

        destino.send('signal', {
            from: origem.id,
            name: origem.name,
            kind: request.kind(),
            payload: request.payload(),
        });

        return new StatusResource('entregue');
    }
}
