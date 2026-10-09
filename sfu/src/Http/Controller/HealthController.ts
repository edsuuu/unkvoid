import type { Request, Response } from 'express';

import { config } from '../../Config/index.js';
import type { RoomRegistry } from '../../Services/RoomRegistry.js';

export class HealthController {
    public constructor(private readonly registry: RoomRegistry) {}

    /**
     * Responder só "o HTTP está de pé" escondia o pior estado: processo vivo, workers
     * mortos e todo join dando 500. Um worker caído enquanto renasce não tira o servidor
     * do ar, porque as salas novas vão para os outros; nenhum vivo, sim.
     */
    public show(_request: Request, response: Response): void {
        const stats = this.registry.stats();
        const ok = stats.workersDown < stats.workers.length;

        response.status(ok ? 200 : 503).json({
            ok,
            appVersion: config.appVersion,
            ...stats,
        });
    }
}
