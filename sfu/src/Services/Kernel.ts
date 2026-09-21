import type { Broadcaster } from './Broadcaster.js';
import type { RoomRegistry } from './RoomRegistry.js';
import type { Subscriptions } from './Subscriptions.js';
import type { ActionName } from '../Enums/Action.js';
import {
    ApiException,
    NotFoundException,
    UnauthorizedException,
} from '../Exceptions/ApiException.js';
import { ConsumerController } from '../Http/Controller/ConsumerController.js';
import { JoinController } from '../Http/Controller/JoinController.js';
import { LeaveController } from '../Http/Controller/LeaveController.js';
import { PeerController } from '../Http/Controller/PeerController.js';
import { ProducerController } from '../Http/Controller/ProducerController.js';
import { SubscriptionController } from '../Http/Controller/SubscriptionController.js';
import { TransportController } from '../Http/Controller/TransportController.js';
import type { Session } from '../Http/Request/Request.js';
import { WebSocketRouter } from '../Routers/WebSocketRouter.js';

export class Kernel {
    private readonly routes: ReturnType<typeof WebSocketRouter.routes>;

    public constructor(
        registry: RoomRegistry,
        subscriptions: Subscriptions,
        broadcaster: Broadcaster,
    ) {
        this.routes = WebSocketRouter.routes({
            join: new JoinController(registry),
            leave: new LeaveController(),
            transport: new TransportController(),
            producer: new ProducerController(),
            peer: new PeerController(),
            consumer: new ConsumerController(),
            subscription: new SubscriptionController(subscriptions, broadcaster),
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

        return route.handle(route.build(data, session) as never);
    }

    public static statusOf(exception: unknown): number {
        return exception instanceof ApiException ? exception.status : 500;
    }
}
