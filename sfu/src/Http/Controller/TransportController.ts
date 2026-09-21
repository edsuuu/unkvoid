import type { Payload } from '../../Routers/WebSocketRouter.js';
import type { Request } from '../Request/Request.js';
import type { TransportRequest } from '../Request/TransportRequest.js';

export class TransportController {
    public async create(request: Request): Promise<Payload> {
        const transport = await request.room().createTransport(request.peer());

        return {
            transportId: transport.id,
            iceParameters: transport.iceParameters,
            iceCandidates: transport.iceCandidates,
            dtlsParameters: transport.dtlsParameters,
        };
    }

    public async connect(request: TransportRequest): Promise<Payload> {
        await request
            .peer()
            .getTransport(request.transportId())
            .connect({ dtlsParameters: request.dtlsParameters() });

        return { status: 'connected' };
    }
}
