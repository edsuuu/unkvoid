import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { Broadcaster } from '../../Services/Broadcaster.js';
import type { RemovePeerRequest } from '../Request/RemovePeerRequest.js';
import type { VoiceStateRequest } from '../Request/VoiceStateRequest.js';

const CHANNEL_ROOM_LENGTH = 26;

const ACCOUNT = /^user:(\d+)$/;

export class PeerController {
    public constructor(private readonly broadcaster: Broadcaster) {}

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

    /**
     * Mutado e ensurdecido são estado de mídia: o Laravel não grava nada disso, então o SFU
     * publica direto no tempo real do canal, para a lista de voz de quem está fora da chamada.
     * Sala por código não tem canal, e quem não é conta não tem `user_id` para a lista.
     */
    public update(request: VoiceStateRequest): Payload {
        const room = request.room();
        const peer = request.peer();
        const changed = peer.muted !== request.muted() || peer.deafened !== request.deafened();
        const account = ACCOUNT.exec(peer.userId);

        peer.muted = request.muted();
        peer.deafened = request.deafened();

        if (changed && account && room.id.length === CHANNEL_ROOM_LENGTH) {
            this.broadcaster.send(`channel.${room.id}`, 'VoiceMuteUpdated', {
                channel_id: room.id,
                user_id: Number(account[1]),
                name: peer.name,
                muted: peer.muted,
                deafened: peer.deafened,
            });
        }

        return { muted: peer.muted, deafened: peer.deafened };
    }

    public ping(): Payload {
        return {};
    }
}
