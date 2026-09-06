import { ApiException, NotFoundException, UnauthorizedException } from '../Exceptions/ApiException.js';
import { ConsumerController } from './Controllers/ConsumerController.js';
import { JoinController } from './Controllers/JoinController.js';
import { ModerationController } from './Controllers/ModerationController.js';
import { ProducerController } from './Controllers/ProducerController.js';
import { TransportController } from './Controllers/TransportController.js';
import { routes } from './routes.js';

export class Kernel {
    constructor(registry, tokens) {
        this.routes = routes({
            join: new JoinController(registry, tokens),
            transport: new TransportController(),
            producer: new ProducerController(),
            consumer: new ConsumerController(),
            moderation: new ModerationController(),
        });
    }

    async dispatch(action, data, session) {
        const route = this.routes[action];

        if (! route) {
            throw new NotFoundException(`ação desconhecida: ${action}`);
        }

        if (! route.guest && ! session.peer) {
            throw new UnauthorizedException('entre em uma sala antes desta ação');
        }

        const resource = await route.handle(new route.request(data, session));

        return resource.toArray();
    }

    static statusOf(exception) {
        return exception instanceof ApiException ? exception.status : 500;
    }
}
