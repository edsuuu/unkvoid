import type { Payload } from '../../Routers/WebSocketRouter.js';
import { Authorizer } from '../../Services/Authorizer.js';
import type { Broadcaster } from '../../Services/Broadcaster.js';
import type { Subscriptions } from '../../Services/Subscriptions.js';
import type { IdentifyRequest } from '../Request/IdentifyRequest.js';
import type { SubscribeRequest } from '../Request/SubscribeRequest.js';

export class SubscriptionController {
    public constructor(
        private readonly subscriptions: Subscriptions,
        private readonly broadcaster: Broadcaster,
    ) {}

    public identify(request: IdentifyRequest): Payload {
        const claims = request.claims();

        request.session.identity = { userId: claims.sub, name: claims.name };

        return { ok: true };
    }

    public async subscribe(request: SubscribeRequest): Promise<Payload> {
        const { userId, name } = request.identity();
        const channel = request.channel();

        const authorized = await Authorizer.allows(userId, channel);
        const subscriber = { socket: request.session.socket, userId, name: authorized ?? name };

        if (!this.subscriptions.add(channel, subscriber)) {
            return { channel, members: this.subscriptions.presence(channel) };
        }

        // Só avisa quando é a primeira conexão daquela pessoa no canal: o app aberto em
        // duas máquinas não pode aparecer entrando duas vezes na lista.
        if (this.subscriptions.countFor(channel, userId) === 1) {
            this.broadcaster.send(
                channel,
                'presence.joining',
                { id: userId, name: subscriber.name },
                request.session.socket,
            );
        }

        return { channel, members: this.subscriptions.presence(channel) };
    }

    public unsubscribe(request: SubscribeRequest): Payload {
        const { userId } = request.identity();
        const channel = request.channel();

        this.subscriptions.remove(channel, request.session.socket);

        if (this.subscriptions.countFor(channel, userId) === 0) {
            this.broadcaster.send(
                channel,
                'presence.leaving',
                { id: userId },
                request.session.socket,
            );
        }

        return { ok: true };
    }
}
