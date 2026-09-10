import type { SetRoomLockRequest } from '../Requests/SetRoomLockRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';

export class RoomController {
    /**
     * Tranca ou destranca a sala.
     *
     * Qualquer pessoa de dentro pode: quem já entrou é confiado por definição, e trancar
     * não age sobre ninguém que esteja lá. A sala inteira é avisada porque uma tranca
     * silenciosa faria a próxima pessoa levar um "não deu para entrar" sem explicação de
     * quem a convidou.
     */
    public setLock(request: SetRoomLockRequest): StatusResource {
        const room = request.room();
        const peer = request.peer();

        room.locked = request.locked();
        room.broadcast('roomLockChanged', { locked: room.locked, byName: peer.name });

        return new StatusResource(room.locked ? 'locked' : 'unlocked');
    }
}
