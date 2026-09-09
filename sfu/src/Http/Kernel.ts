import type { ActionName } from '../Enums/Action.js';
import {
    ApiException,
    NotFoundException,
    UnauthorizedException,
} from '../Exceptions/ApiException.js';
import type { RoomRegistry } from '../Services/RoomRegistry.js';
import type { Session } from '../types.js';
import { ConsumerController } from './Controllers/ConsumerController.js';
import { JoinController } from './Controllers/JoinController.js';
import { LeaveController } from './Controllers/LeaveController.js';
import { PeerController } from './Controllers/PeerController.js';
import { ProducerController } from './Controllers/ProducerController.js';
import { TransportController } from './Controllers/TransportController.js';
import { routes } from './routes.js';

export class Kernel {
    private readonly routes: ReturnType<typeof routes>;

    public constructor(registry: RoomRegistry) {
        this.routes = routes({
            join: new JoinController(registry),
            leave: new LeaveController(),
            transport: new TransportController(),
            producer: new ProducerController(),
            peer: new PeerController(),
            consumer: new ConsumerController(),
        });
    }

    public async dispatch(
        action: string,
        data: Record<string, unknown> | undefined,
        session: Session,
    ): Promise<Record<string, unknown>> {
        const route = this.routes[action as ActionName];

        if (!route) {
            throw new NotFoundException(`unknown action: ${action}`);
        }

        if (!route.guest && !session.peer) {
            throw new UnauthorizedException('join a room before this action');
        }

        const resource = await route.handle(route.build(data, session) as never);

        return resource.toArray();
    }

    public static statusOf(exception: unknown): number {
        return exception instanceof ApiException ? exception.status : 500;
    }
}
