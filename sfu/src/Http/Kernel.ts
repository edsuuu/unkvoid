import type { ActionName } from '../Enums/Action.js';
import { ApiException, NotFoundException, UnauthorizedException } from '../Exceptions/ApiException.js';
import type { PresenceRegistry } from '../Services/PresenceRegistry.js';
import type { RoomRegistry } from '../Services/RoomRegistry.js';
import type { TokenVerifier } from '../Services/TokenVerifier.js';
import { ConsumerController } from './Controllers/ConsumerController.js';
import { JoinController } from './Controllers/JoinController.js';
import { LeaveController } from './Controllers/LeaveController.js';
import { PresenceController } from './Controllers/PresenceController.js';
import { ModerationController } from './Controllers/ModerationController.js';
import { ProducerController } from './Controllers/ProducerController.js';
import { SignalController } from './Controllers/SignalController.js';
import { TransportController } from './Controllers/TransportController.js';
import { routes } from './routes.js';
import type { Session } from '../types.js';

export class Kernel {
    private readonly routes: ReturnType<typeof routes>;

    constructor(registry: RoomRegistry, tokens: TokenVerifier, presence: PresenceRegistry) {
        this.routes = routes({
            join: new JoinController(registry, tokens, presence),
            leave: new LeaveController((roomId, peerId) => presence.leave(roomId, peerId)),
            presence: new PresenceController(presence, tokens),
            transport: new TransportController(),
            producer: new ProducerController(presence),
            signal: new SignalController(),
            consumer: new ConsumerController(),
            moderation: new ModerationController((roomId, peerId) => presence.setSharing(roomId, peerId, false)),
        });
    }

    async dispatch(action: string, data: Record<string, unknown> | undefined, session: Session): Promise<Record<string, unknown>> {
        const route = this.routes[action as ActionName];

        if (! route) {
            throw new NotFoundException(`ação desconhecida: ${action}`);
        }

        if (! route.guest && ! session.peer) {
            throw new UnauthorizedException('entre em uma sala antes desta ação');
        }

        const resource = await route.handle(route.build(data, session) as never);

        return resource.toArray();
    }

    static statusOf(exception: unknown): number {
        return exception instanceof ApiException ? exception.status : 500;
    }
}
