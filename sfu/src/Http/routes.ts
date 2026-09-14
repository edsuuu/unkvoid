import { Action, type ActionName } from '../Enums/Action.js';
import type { Resource, Session } from '../types.js';
import type { ConsumerController } from './Controllers/ConsumerController.js';
import type { JoinController } from './Controllers/JoinController.js';
import type { LeaveController } from './Controllers/LeaveController.js';
import type { PeerController } from './Controllers/PeerController.js';
import type { ProducerController } from './Controllers/ProducerController.js';
import type { TransportController } from './Controllers/TransportController.js';
import { ConsumePlainRequest } from './Requests/ConsumePlainRequest.js';
import { ConsumeRequest } from './Requests/ConsumeRequest.js';
import { ConsumerRequest } from './Requests/ConsumerRequest.js';
import { JoinRequest } from './Requests/JoinRequest.js';
import { ProducePlainRequest } from './Requests/ProducePlainRequest.js';
import { ProduceRequest } from './Requests/ProduceRequest.js';
import { ProducerRequest } from './Requests/ProducerRequest.js';
import { RemovePeerRequest } from './Requests/RemovePeerRequest.js';
import { Request } from './Requests/Request.js';
import { TransportRequest } from './Requests/TransportRequest.js';

export type Controllers = {
    join: JoinController;
    leave: LeaveController;
    transport: TransportController;
    producer: ProducerController;
    peer: PeerController;
    consumer: ConsumerController;
};

type Route = {
    guest?: boolean;
    build: (data: Record<string, unknown> | undefined, session: Session) => Request;
    handle: (request: never) => Resource | Promise<Resource>;
};

/**
 * As rotas da API do SFU. `guest: true` é a única ação aberta — todas as outras
 * exigem sessão, como faria um middleware de autenticação.
 */
export const routes = (controllers: Controllers): Record<ActionName, Route> => ({
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
});
