import type { Request, Response } from 'express';

import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Broadcaster } from '../../Services/Broadcaster.js';

export class BroadcastController {
    public constructor(private readonly broadcaster: Broadcaster) {}

    public store(request: Request, response: Response): void {
        const { channel, event, data } = request.body as {
            channel?: unknown;
            event?: unknown;
            data?: unknown;
        };

        if (typeof channel !== 'string' || channel === '') {
            throw new ValidationException('field channel is required');
        }

        if (typeof event !== 'string' || event === '') {
            throw new ValidationException('field event is required');
        }

        response.json({ delivered: this.broadcaster.send(channel, event, data ?? {}) });
    }
}
