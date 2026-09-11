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
     * Um worker do mediasoup é um PROCESSO C++ separado e de uma thread só — ele satura
     * um núcleo e para por ali. Threads do Node não ajudariam: mídia nunca passa pelo
     * JavaScript, só a sinalização passa. Escalar aqui é um worker por núcleo e
     * distribuir as salas entre eles.
     *
     * Cada worker precisa da própria porta de mídia porque o WebRtcServer não é
     * compartilhado entre processos.
     */
    public async boot(): Promise<void> {
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
                    {
                        protocol: 'udp',
                        ip: '0.0.0.0',
                        announcedAddress: config.announcedAddress,
                        port,
                    },
                    {
                        protocol: 'tcp',
                        ip: '0.0.0.0',
                        announcedAddress: config.announcedAddress,
                        port,
                    },
                ],
            });

            this.slots.push({ worker, webRtcServer, rooms: 0 });
        }

        const lastPlain =
            config.plainPortBase + config.workerCount * config.plainPortsPerWorker - 1;

        console.log(
            `[INFO] ${this.slots.length} media workers on ports ${config.mediaPort}-${config.mediaPort + this.slots.length - 1} · plain RTP on ${config.plainPortBase}-${lastPlain}`,
        );
    }

    /** Sala nova vai para o worker com menos salas. */
    private leastLoadedSlot(): WorkerSlot {
        return this.slots.reduce((smallest, slot) =>
            slot.rooms < smallest.rooms ? slot : smallest,
        );
    }

    public find(roomId: string): Room | undefined {
        return this.rooms.get(roomId);
    }

    public async findOrCreate(roomId: string): Promise<Room> {
        const existing = this.rooms.get(roomId);

        if (existing) {
            return existing;
        }

        if (this.slots.length === 0) {
            throw new Error('the room registry was not initialized');
        }

        const slot = this.leastLoadedSlot();
        const room = await Room.create(slot.worker, slot.webRtcServer, roomId);

        room.onEvicted = (empty) => this.release(empty);

        slot.rooms += 1;
        this.rooms.set(roomId, room);
        this.slotByRoom.set(roomId, slot);

        return room;
    }

    public release(room: Room): void {
        if (room.activeCount() > 0 || !room.isEmpty()) {
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

    public stats(): { rooms: number; peers: number; workers: number[] } {
        return {
            rooms: this.rooms.size,
            peers: [...this.rooms.values()].reduce((total, room) => total + room.peers.size, 0),
            workers: this.slots.map((slot) => slot.rooms),
        };
    }
}
