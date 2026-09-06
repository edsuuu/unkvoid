import { StatusResource } from '../Resources/StatusResource.js';
import { TransportResource } from '../Resources/TransportResource.js';

export class TransportController {
    async create(request) {
        return new TransportResource(await request.room().createTransport(request.peer()));
    }

    async connect(request) {
        await request.peer()
            .getTransport(request.transportId())
            .connect({ dtlsParameters: request.dtlsParameters() });

        return new StatusResource('connected');
    }
}
