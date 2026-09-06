import { Action } from '../Enums/Action.js';
import { ConsumeRequest } from './Requests/ConsumeRequest.js';
import { ConsumerRequest } from './Requests/ConsumerRequest.js';
import { JoinRequest } from './Requests/JoinRequest.js';
import { ModerationRequest } from './Requests/ModerationRequest.js';
import { ProduceRequest } from './Requests/ProduceRequest.js';
import { ProducerRequest } from './Requests/ProducerRequest.js';
import { Request } from './Requests/Request.js';
import { TransportRequest } from './Requests/TransportRequest.js';

/**
 * Rotas da API do SFU. `guest: true` é a única ação aberta — todas as outras
 * exigem sessão, do mesmo jeito que um middleware de auth faria.
 */
export const routes = controllers => ({
    [Action.Join]: {
        guest: true,
        request: JoinRequest,
        handle: request => controllers.join.__invoke(request),
    },
    [Action.CreateTransport]: {
        request: Request,
        handle: request => controllers.transport.create(request),
    },
    [Action.ConnectTransport]: {
        request: TransportRequest,
        handle: request => controllers.transport.connect(request),
    },
    [Action.Produce]: {
        request: ProduceRequest,
        handle: request => controllers.producer.store(request),
    },
    [Action.CloseProducer]: {
        request: ProducerRequest,
        handle: request => controllers.producer.destroy(request),
    },
    [Action.Consume]: {
        request: ConsumeRequest,
        handle: request => controllers.consumer.store(request),
    },
    [Action.ResumeConsumer]: {
        request: ConsumerRequest,
        handle: request => controllers.consumer.resume(request),
    },
    [Action.SetPreferredLayers]: {
        request: ConsumerRequest,
        handle: request => controllers.consumer.setPreferredLayers(request),
    },
    [Action.StopBroadcast]: {
        request: ModerationRequest,
        handle: request => controllers.moderation.stopBroadcast(request),
    },
    [Action.KickPeer]: {
        request: ModerationRequest,
        handle: request => controllers.moderation.kick(request),
    },
});
