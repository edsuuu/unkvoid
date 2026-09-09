import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';

export class RemovePeerRequest extends Request {
    public peerId(): string {
        return this.string('peerId');
    }

    public target(room: Room): Peer {
        const target = room.peers.get(this.peerId());

        if (!target) {
            throw new ValidationException('participant does not exist');
        }

        return target;
    }
}
