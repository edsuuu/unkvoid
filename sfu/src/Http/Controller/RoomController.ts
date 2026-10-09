import type { Request, Response } from 'express';

import { ValidationException } from '../../Exceptions/ApiException.js';
import type { Move } from '../../Services/Room.js';
import type { RoomRegistry } from '../../Services/RoomRegistry.js';

const ROOM_ID = /^[a-z0-9-]+$/;

const CHANNEL_ID = /^[a-z0-9]{26}$/;

const MAX_NAME = 64;

export class RoomController {
    public constructor(private readonly registry: RoomRegistry) {}

    public kick(request: Request, response: Response): void {
        const room = this.registry.find(this.roomId(request));

        response.json({ kicked: room?.kickUser(this.userId(request), this.move(request)) ?? 0 });
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

    private move(request: Request): Move | null {
        const { to, by } = request.body as { to?: unknown; by?: unknown };

        if (to === undefined || to === null) {
            return null;
        }

        if (typeof to !== 'string' || !CHANNEL_ID.test(to)) {
            throw new ValidationException('field to must be a channel id');
        }

        if (by !== undefined && by !== null && (typeof by !== 'string' || by.length > MAX_NAME)) {
            throw new ValidationException(`field by must be a name up to ${MAX_NAME} characters`);
        }

        return { to, by: typeof by === 'string' ? by : null };
    }

    private userId(request: Request): string {
        const { userId } = request.body as { userId?: unknown };

        if (typeof userId !== 'string' || userId === '') {
            throw new ValidationException('field userId is required');
        }

        return userId;
    }
}
