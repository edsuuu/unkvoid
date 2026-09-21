import { Action, type ActionName } from '../Enums/Action.js';
import type { ConsumerController } from '../Http/Controller/ConsumerController.js';
import type { JoinController } from '../Http/Controller/JoinController.js';
import type { LeaveController } from '../Http/Controller/LeaveController.js';
import type { PeerController } from '../Http/Controller/PeerController.js';
import type { ProducerController } from '../Http/Controller/ProducerController.js';
import type { SubscriptionController } from '../Http/Controller/SubscriptionController.js';
import type { TransportController } from '../Http/Controller/TransportController.js';
import { ConsumePlainRequest } from '../Http/Request/ConsumePlainRequest.js';
import { ConsumeRequest } from '../Http/Request/ConsumeRequest.js';
import { ConsumerRequest } from '../Http/Request/ConsumerRequest.js';
import { IdentifyRequest } from '../Http/Request/IdentifyRequest.js';
import { JoinRequest } from '../Http/Request/JoinRequest.js';
import { ProducePlainRequest } from '../Http/Request/ProducePlainRequest.js';
import { ProduceRequest } from '../Http/Request/ProduceRequest.js';
import { ProducerRequest } from '../Http/Request/ProducerRequest.js';
import { RemovePeerRequest } from '../Http/Request/RemovePeerRequest.js';
import { Request } from '../Http/Request/Request.js';
import type { Session } from '../Http/Request/Request.js';
import { SubscribeRequest } from '../Http/Request/SubscribeRequest.js';
import { TransportRequest } from '../Http/Request/TransportRequest.js';

export type Payload = Record<string, unknown>;

export type Controllers = {
    join: JoinController;
    leave: LeaveController;
    transport: TransportController;
    producer: ProducerController;
    peer: PeerController;
    consumer: ConsumerController;
    subscription: SubscriptionController;
};

type Route = {
    guest?: boolean;
    build: (data: Record<string, unknown> | undefined, session: Session) => Request;
    handle: (request: never) => Payload | Promise<Payload>;
};

export class WebSocketRouter {
    public static routes(controllers: Controllers): Record<ActionName, Route> {
        return {
            [Action.Join]: {
                guest: true,
                build: (data, session) => new JoinRequest(data, session),
                handle: (request) => controllers.join.handle(request),
            },
            [Action.Leave]: {
                build: (data, session) => new Request(data, session),
                handle: (request) => controllers.leave.handle(request),
            },
            [Action.RemovePeer]: {
                build: (data, session) => new RemovePeerRequest(data, session),
                handle: (request) => controllers.peer.remove(request),
            },
            [Action.Ping]: {
                guest: true,
                build: (data, session) => new Request(data, session),
                handle: () => controllers.peer.ping(),
            },
            [Action.Identify]: {
                guest: true,
                build: (data, session) => new IdentifyRequest(data, session),
                handle: (request) => controllers.subscription.identify(request),
            },
            [Action.Subscribe]: {
                guest: true,
                build: (data, session) => new SubscribeRequest(data, session),
                handle: (request) => controllers.subscription.subscribe(request),
            },
            [Action.Unsubscribe]: {
                guest: true,
                build: (data, session) => new SubscribeRequest(data, session),
                handle: (request) => controllers.subscription.unsubscribe(request),
            },
            [Action.CreateTransport]: {
                build: (data, session) => new Request(data, session),
                handle: (request) => controllers.transport.create(request),
            },
            [Action.ConnectTransport]: {
                build: (data, session) => new TransportRequest(data, session),
                handle: (request) => controllers.transport.connect(request),
            },
            [Action.Produce]: {
                build: (data, session) => new ProduceRequest(data, session),
                handle: (request) => controllers.producer.store(request),
            },
            [Action.ProducePlain]: {
                build: (data, session) => new ProducePlainRequest(data, session),
                handle: (request) => controllers.producer.storePlain(request),
            },
            [Action.PauseProducer]: {
                build: (data, session) => new ProducerRequest(data, session),
                handle: (request) => controllers.producer.pause(request),
            },
            [Action.ResumeProducer]: {
                build: (data, session) => new ProducerRequest(data, session),
                handle: (request) => controllers.producer.resume(request),
            },
            [Action.CloseProducer]: {
                build: (data, session) => new ProducerRequest(data, session),
                handle: (request) => controllers.producer.destroy(request),
            },
            [Action.Consume]: {
                build: (data, session) => new ConsumeRequest(data, session),
                handle: (request) => controllers.consumer.store(request),
            },
            [Action.ConsumePlain]: {
                build: (data, session) => new ConsumePlainRequest(data, session),
                handle: (request) => controllers.consumer.storePlain(request),
            },
            [Action.ResumeConsumer]: {
                build: (data, session) => new ConsumerRequest(data, session),
                handle: (request) => controllers.consumer.resume(request),
            },
            [Action.PauseConsumer]: {
                build: (data, session) => new ConsumerRequest(data, session),
                handle: (request) => controllers.consumer.pause(request),
            },
            [Action.CloseConsumer]: {
                build: (data, session) => new ConsumerRequest(data, session),
                handle: (request) => controllers.consumer.destroy(request),
            },
        };
    }
}
