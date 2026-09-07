import * as mediasoup from 'mediasoup';
import type { WebRtcServer, Worker } from 'mediasoup/types';

import { config } from '../config.js';
import { Room } from './Room.js';

type WorkerSlot = { worker: Worker; webRtcServer: WebRtcServer; rooms: number };

export class RoomRegistry {
    private readonly slots: WorkerSlot[] = [];

    private readonly rooms = new Map<string, Room>();

    private readonly slotByRoom = new Map<string, WorkerSlot>();

    /**
     * A mediasoup worker is a separate, single-threaded C++ PROCESS — it saturates
     * one core and stops. Node threads would not help: media never passes through
     * JavaScript, only signaling does. Scaling here means one worker per core and
     * distribuir as salas entre eles.
     *
     * Each worker needs its own media port because WebRtcServer is not
     * shared between processes.
     */
    async boot(): Promise<void> {
        for (let index = 0; index < config.workerCount; index += 1) {
            const rtcMinPort = config.plainPortBase + index * config.plainPortsPerWorker;

            const worker = await mediasoup.createWorker({
                ...config.worker,
                rtcMinPort,
                rtcMaxPort: rtcMinPort + config.plainPortsPerWorker - 1,
            });

            worker.on('died', () => {
                console.error('[ERROR] mediasoup worker died — exiting so pm2 can restart');
                process.exit(1);
            });

            const port = config.mediaPort + index;

            const webRtcServer = await worker.createWebRtcServer({
                listenInfos: [
                    { protocol: 'udp', ip: '0.0.0.0', announcedAddress: config.announcedAddress, port },
                    { protocol: 'tcp', ip: '0.0.0.0', announcedAddress: config.announcedAddress, port },
                ],
            });

            this.slots.push({ worker, webRtcServer, rooms: 0 });
        }

        const lastPlain = config.plainPortBase + config.workerCount * config.plainPortsPerWorker - 1;

        console.log(`[INFO] ${this.slots.length} media workers on ports ${config.mediaPort}-${config.mediaPort + this.slots.length - 1} · plain RTP on ${config.plainPortBase}-${lastPlain}`);
    }

    /** A new room goes to the worker with the fewest rooms. */
    private leastLoadedSlot(): WorkerSlot {
        return this.slots.reduce((smallest, slot) => (slot.rooms < smallest.rooms ? slot : smallest));
    }

    async findOrCreate(roomId: string): Promise<Room> {
        const existing = this.rooms.get(roomId);

        if (existing) {
            return existing;
        }

        if (this.slots.length === 0) {
            throw new Error('the room registry was not initialized');
        }

        const slot = this.leastLoadedSlot();
        const room = await Room.create(slot.worker, slot.webRtcServer, roomId);

        slot.rooms += 1;
        this.rooms.set(roomId, room);
        this.slotByRoom.set(roomId, slot);

        return room;
    }

    release(room: Room): void {
        if (room.activeCount() > 0 || ! room.isEmpty()) {
            return;
        }

        const slot = this.slotByRoom.get(room.id);

        if (slot) {
            slot.rooms -= 1;
            this.slotByRoom.delete(room.id);
        }

        room.close();
        this.rooms.delete(room.id);
    }

    stats(): { rooms: number; peers: number; workers: number[] } {
        return {
            rooms: this.rooms.size,
            peers: [...this.rooms.values()].reduce((total, room) => total + room.peers.size, 0),
            workers: this.slots.map(slot => slot.rooms),
        };
    }
}
