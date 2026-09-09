import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Peer } from '../../Services/Peer.js';
import type { Room } from '../../Services/Room.js';
import { Request } from './Request.js';

export class RemovePeerRequest extends Request {
    peerId(): string {
        return this.string('peerId');
    }

    target(room: Room): Peer {
        const target = room.peers.get(this.peerId());

        if (! target) {
            throw new ValidationException('participant does not exist');
        }

        return target;
    }
}
