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
        const roomId = this.roomId(request);
        const userId = this.userId(request);
        const move = this.move(request);

        this.registry.bar(roomId, userId);
        response.json({ kicked: this.registry.find(roomId)?.kickUser(userId, move) ?? 0 });
    }

    /**
     * As salas vêm no corpo (os canais de voz do servidor): expulsar e banir não dependem de o
     * Laravel saber em qual delas a pessoa está, saem numa chamada só, e nunca alcançam a voz
     * de outro servidor — a conta tem uma sessão no SFU inteiro, e ela pode estar lá.
     */
    public kickIn(request: Request, response: Response): void {
        const userId = this.userId(request);
        let kicked = 0;

        for (const roomId of this.rooms(request)) {
            this.registry.bar(roomId, userId);
            kicked += this.registry.find(roomId)?.kickUser(userId) ?? 0;
        }

        response.json({ kicked });
    }

    public async mute(request: Request, response: Response): Promise<void> {
        const room = this.registry.find(this.roomId(request));

        response.json({
            muted: (await room?.muteUser(this.userId(request), this.muted(request))) ?? 0,
        });
    }

    public async muteIn(request: Request, response: Response): Promise<void> {
        const userId = this.userId(request);
        const muted = this.muted(request);
        let touched = 0;

        for (const roomId of this.rooms(request)) {
            touched += (await this.registry.find(roomId)?.muteUser(userId, muted)) ?? 0;
        }

        response.json({ muted: touched });
    }

    public presence(_request: Request, response: Response): void {
        response.json({ rooms: this.registry.presence() });
    }

    private rooms(request: Request): string[] {
        const { rooms } = request.body as { rooms?: unknown };

        if (
            !Array.isArray(rooms) ||
            rooms.length === 0 ||
            !rooms.every((room) => typeof room === 'string' && ROOM_ID.test(room))
        ) {
            throw new ValidationException('field rooms must be a list of room ids');
        }

        return rooms as string[];
    }

    private muted(request: Request): boolean {
        const { muted } = request.body as { muted?: unknown };

        if (typeof muted !== 'boolean') {
            throw new ValidationException('field muted must be true or false');
        }

        return muted;
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
