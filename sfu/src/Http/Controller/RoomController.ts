import type { Request, Response } from 'express';

import { ValidationException } from '../../Exceptions/ApiException.js';
import type { RoomRegistry } from '../../Services/RoomRegistry.js';

const ROOM_ID = /^[a-z0-9-]+$/;

export class RoomController {
    public constructor(private readonly registry: RoomRegistry) {}

    public kick(request: Request, response: Response): void {
        const room = this.registry.find(this.roomId(request));

        response.json({ kicked: room?.kickUser(this.userId(request)) ?? 0 });
    }

    public async mute(request: Request, response: Response): Promise<void> {
        const { muted } = request.body as { muted?: unknown };

        if (typeof muted !== 'boolean') {
            throw new ValidationException('field muted must be true or false');
        }

        const room = this.registry.find(this.roomId(request));

        response.json({
            muted: (await room?.muteUser(this.userId(request), muted)) ?? 0,
        });
    }

    public presence(_request: Request, response: Response): void {
        response.json({ rooms: this.registry.presence() });
    }

    private roomId(request: Request): string {
        const room = String(request.params.room ?? '');

        if (!ROOM_ID.test(room)) {
            throw new ValidationException('field room is malformed');
        }

        return room;
    }

    private userId(request: Request): string {
        const { userId } = request.body as { userId?: unknown };

        if (typeof userId !== 'string' || userId === '') {
            throw new ValidationException('field userId is required');
        }

        return userId;
    }
}
