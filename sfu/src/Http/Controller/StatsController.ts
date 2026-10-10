import type { Request, Response } from 'express';

import type { RoomRegistry } from '../../Services/RoomRegistry.js';

export class StatsController {
    public constructor(private readonly registry: RoomRegistry) {}

    /**
     * O que o mediasoup tem aberto de verdade, worker por worker, contado nos `dump()` dele e
     * não nos mapas do `Peer`: o vazamento que importa é justamente o objeto que o SFU
     * esqueceu e o worker continua segurando.
     */
    public async show(_request: Request, response: Response): Promise<void> {
        response.json(await this.registry.dump());
    }
}
