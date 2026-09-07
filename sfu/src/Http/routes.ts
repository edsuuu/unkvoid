import { Action, type ActionName } from '../Enums/Action.js';
import type { Resource, Session } from '../types.js';
import type { ConsumerController } from './Controllers/ConsumerController.js';
import type { JoinController } from './Controllers/JoinController.js';
import type { LeaveController } from './Controllers/LeaveController.js';
import type { PresenceController } from './Controllers/PresenceController.js';
import type { ModerationController } from './Controllers/ModerationController.js';
import type { ProducerController } from './Controllers/ProducerController.js';
import type { SignalController } from './Controllers/SignalController.js';
import type { TransportController } from './Controllers/TransportController.js';
import { ConsumeRequest } from './Requests/ConsumeRequest.js';
import { ConsumerRequest } from './Requests/ConsumerRequest.js';
import { JoinRequest } from './Requests/JoinRequest.js';
import { ModerationRequest } from './Requests/ModerationRequest.js';
import { ProduceRequest } from './Requests/ProduceRequest.js';
import { ProducePlainRequest } from './Requests/ProducePlainRequest.js';
import { ProducerRequest } from './Requests/ProducerRequest.js';
import { Request } from './Requests/Request.js';
import { SignalRequest } from './Requests/SignalRequest.js';
import { TransportRequest } from './Requests/TransportRequest.js';

export type Controllers = {
    join: JoinController;
    leave: LeaveController;
    presence: PresenceController;
    signal: SignalController;
    transport: TransportController;
    producer: ProducerController;
    consumer: ConsumerController;
    moderation: ModerationController;
};

type Route = {
    guest?: boolean;
    build: (data: Record<string, unknown> | undefined, session: Session) => Request;
    handle: (request: never) => Resource | Promise<Resource>;
};

/**
 * SFU API routes. `guest: true` is the only open action — all others
 * require a session, just as an auth middleware would.
 */
export const routes = (controllers: Controllers): Record<ActionName, Route> => ({
    [Action.Join]: {
        guest: true,
        build: (data, session) => new JoinRequest(data, session),
        handle: request => controllers.join.handle(request),
    },
    [Action.Leave]: {
        build: (data, session) => new Request(data, session),
        handle: request => controllers.leave.handle(request),
    },
    [Action.WatchServer]: {
        guest: true,
        build: (data, session) => new JoinRequest(data, session),
        handle: request => controllers.presence.watch(request),
    },
    [Action.Signal]: {
        build: (data, session) => new SignalRequest(data, session),
        handle: request => controllers.signal.handle(request),
    },
    [Action.CreateTransport]: {
        build: (data, session) => new Request(data, session),
        handle: request => controllers.transport.create(request),
    },
    [Action.ConnectTransport]: {
        build: (data, session) => new TransportRequest(data, session),
        handle: request => controllers.transport.connect(request),
    },
    [Action.Produce]: {
        build: (data, session) => new ProduceRequest(data, session),
        handle: request => controllers.producer.store(request),
    },
    [Action.ProducePlain]: {
        build: (data, session) => new ProducePlainRequest(data, session),
        handle: request => controllers.producer.storePlain(request),
    },
    [Action.CloseProducer]: {
        build: (data, session) => new ProducerRequest(data, session),
        handle: request => controllers.producer.destroy(request),
    },
    [Action.Consume]: {
        build: (data, session) => new ConsumeRequest(data, session),
        handle: request => controllers.consumer.store(request),
    },
    [Action.ResumeConsumer]: {
        build: (data, session) => new ConsumerRequest(data, session),
        handle: request => controllers.consumer.resume(request),
    },
    [Action.PauseConsumer]: {
        build: (data, session) => new ConsumerRequest(data, session),
        handle: request => controllers.consumer.pause(request),
    },
    [Action.SetPreferredLayers]: {
        build: (data, session) => new ConsumerRequest(data, session),
        handle: request => controllers.consumer.setPreferredLayers(request),
    },
    [Action.StopBroadcast]: {
        build: (data, session) => new ModerationRequest(data, session),
        handle: request => controllers.moderation.stopBroadcast(request),
    },
    [Action.DisconnectPeer]: {
        build: (data, session) => new ModerationRequest(data, session),
        handle: request => controllers.moderation.disconnect(request),
    },
});
