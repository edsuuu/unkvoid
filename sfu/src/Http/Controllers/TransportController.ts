import type { Request } from '../Requests/Request.js';
import type { TransportRequest } from '../Requests/TransportRequest.js';
import { StatusResource } from '../Resources/StatusResource.js';
import { TransportResource } from '../Resources/TransportResource.js';

export class TransportController {
    public async create(request: Request): Promise<TransportResource> {
        return new TransportResource(await request.room().createTransport(request.peer()));
    }

    public async connect(request: TransportRequest): Promise<StatusResource> {
        await request
            .peer()
            .getTransport(request.transportId())
            .connect({ dtlsParameters: request.dtlsParameters() });

        return new StatusResource('connected');
    }
}
