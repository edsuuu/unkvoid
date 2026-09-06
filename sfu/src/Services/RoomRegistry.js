import * as mediasoup from 'mediasoup';

import { config } from '../config.js';
import { Room } from './Room.js';

export class RoomRegistry {
    constructor() {
        this.worker = null;
        this.webRtcServer = null;
        this.rooms = new Map();
    }

    async boot() {
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

    async findOrCreate(roomId) {
        const existing = this.rooms.get(roomId);

        if (existing) {
            return existing;
        }

        const room = await Room.create(this.worker, this.webRtcServer, roomId);

        this.rooms.set(roomId, room);

        return room;
    }

    release(room) {
        if (! room.isEmpty()) {
            return;
        }

        room.close();
        this.rooms.delete(room.id);
    }

    stats() {
        return {
            rooms: this.rooms.size,
            peers: [...this.rooms.values()].reduce((total, room) => total + room.peers.size, 0),
        };
    }
}
