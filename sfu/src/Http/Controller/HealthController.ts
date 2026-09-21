import type { Request, Response } from 'express';

import { config } from '../../Config/index.js';
import type { RoomRegistry } from '../../Services/RoomRegistry.js';

export class HealthController {
    public constructor(private readonly registry: RoomRegistry) {}

    public show(_request: Request, response: Response): void {
        response.json({
            ok: true,
            appVersion: config.appVersion,
            ...this.registry.stats(),
        });
    }
}
