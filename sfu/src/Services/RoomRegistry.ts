import * as mediasoup from 'mediasoup';
import type { WebRtcServer, Worker } from 'mediasoup/types';

import { config } from '../config.js';
import { Room } from './Room.js';

export class RoomRegistry {
    private worker: Worker | null = null;

    private webRtcServer: WebRtcServer | null = null;

    private readonly rooms = new Map<string, Room>();

    async boot(): Promise<void> {
        this.worker = await mediasoup.createWorker(config.worker);

        this.worker.on('died', () => {
            console.error('[ERRO] worker do mediasoup morreu — saindo para o pm2 reiniciar');
            process.exit(1);
        });

        this.webRtcServer = await this.worker.createWebRtcServer({
            listenInfos: [
                { protocol: 'udp', ip: '0.0.0.0', announcedAddress: config.announcedAddress, port: config.mediaPort },
                { protocol: 'tcp', ip: '0.0.0.0', announcedAddress: config.announcedAddress, port: config.mediaPort },
            ],
        });
    }

    async findOrCreate(roomId: string): Promise<Room> {
        const existing = this.rooms.get(roomId);

        if (existing) {
            return existing;
        }

        if (!this.worker || !this.webRtcServer) {
            throw new Error('o registro de salas não foi inicializado');
        }

        const room = await Room.create(this.worker, this.webRtcServer, roomId);

        this.rooms.set(roomId, room);

        return room;
    }

    release(room: Room): void {
        if (! room.isEmpty()) {
            return;
        }

        room.close();
        this.rooms.delete(room.id);
    }

    stats(): { rooms: number; peers: number } {
        return {
            rooms: this.rooms.size,
            peers: [...this.rooms.values()].reduce((total, room) => total + room.peers.size, 0),
        };
    }
}
