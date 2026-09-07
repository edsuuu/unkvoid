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
     * Um worker do mediasoup é um PROCESSO C++ separado e single-thread — ele satura
     * um núcleo e para. Threads no Node não ajudariam: a mídia nunca passa pelo
     * JavaScript, só a sinalização. Escalar aqui é ter um worker por núcleo e
     * distribuir as salas entre eles.
     *
     * Cada worker precisa da própria porta de mídia, porque o WebRtcServer não é
     * compartilhável entre processos.
     */
    async boot(): Promise<void> {
        for (let index = 0; index < config.workerCount; index += 1) {
            const worker = await mediasoup.createWorker(config.worker);

            worker.on('died', () => {
                console.error('[ERRO] worker do mediasoup morreu — saindo para o pm2 reiniciar');
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

        console.log(`[INFO] ${this.slots.length} workers de mídia nas portas ${config.mediaPort}-${config.mediaPort + this.slots.length - 1}`);
    }

    /** Sala nova vai para o worker com menos salas. */
    private leastLoadedSlot(): WorkerSlot {
        return this.slots.reduce((menor, slot) => (slot.rooms < menor.rooms ? slot : menor));
    }

    async findOrCreate(roomId: string): Promise<Room> {
        const existing = this.rooms.get(roomId);

        if (existing) {
            return existing;
        }

        if (this.slots.length === 0) {
            throw new Error('o registro de salas não foi inicializado');
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
