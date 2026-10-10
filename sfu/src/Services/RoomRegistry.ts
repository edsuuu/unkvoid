import * as mediasoup from 'mediasoup';
import type { Router, WebRtcServer, Worker } from 'mediasoup/types';

import type { Peer } from './Peer.js';
import { Room, type MediaRouter } from './Room.js';
import { config } from '../Config/index.js';
import { ServiceUnavailableException } from '../Exceptions/ApiException.js';

type WorkerSlot = { worker: Worker; webRtcServer: WebRtcServer; routers: Set<Router> };

export type WorkerDump = {
    index: number;
    pid: number;
    closed: boolean;
    routers: number;
    transports: number;
    producers: number;
    consumers: number;
    maxRssKb: number;
    cpuMs: number;
};

const RESPAWN_CEILING_MS = 30_000;

export class RoomRegistry {
    private readonly slots: WorkerSlot[] = [];

    private readonly rooms = new Map<string, Room>();

    private readonly creating = new Map<string, Promise<Room>>();

    public async boot(): Promise<void> {
        for (let index = 0; index < config.workerCount; index += 1) {
            this.slots.push(await this.spawn(index));
        }

        const lastPlain =
            config.plainPortBase + config.workerCount * config.plainPortsPerWorker - 1;

        console.log(
            `[INFO] ${this.slots.length} media workers on ports ${config.mediaPort}-${config.mediaPort + this.slots.length - 1} · plain RTP on ${config.plainPortBase}-${lastPlain}`,
        );
    }

    private async spawn(index: number): Promise<WorkerSlot> {
        const rtcMinPort = config.plainPortBase + index * config.plainPortsPerWorker;

        const worker = await mediasoup.createWorker({
            ...config.worker,
            rtcMinPort,
            rtcMaxPort: rtcMinPort + config.plainPortsPerWorker - 1,
        });

        const port = config.mediaPort + index;

        const webRtcServer = await worker
            .createWebRtcServer({
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
            })
            .catch((failure: unknown) => {
                worker.close();
                throw failure;
            });

        const slot: WorkerSlot = { worker, webRtcServer, routers: new Set() };

        worker.on('died', (error) => this.revive(index, slot, error));

        return slot;
    }

    /**
     * Antes, worker morto derrubava o processo inteiro, e com ele as salas dos workers
     * sãos. Agora só quem estava nele cai: o socket fechado faz cada app voltar sozinho, e a
     * volta cai num worker vivo — este mesmo, se já tiver renascido.
     */
    private revive(index: number, dead: WorkerSlot, error: Error): void {
        const rooms = [...this.rooms.values()].filter((room) => room.uses(dead.worker));

        console.error(
            `[ERROR] media worker ${index} died (${error.message}) · closing it in ${rooms.length} rooms`,
        );

        for (const room of rooms) {
            room.evacuate(dead.worker);
            this.release(room);
        }

        const attempt = (delay: number): void => {
            this.spawn(index)
                .then((slot) => {
                    this.slots[index] = slot;
                    console.log(`[INFO] media worker ${index} is back (pid ${slot.worker.pid})`);
                })
                .catch((failure: unknown) => {
                    console.error(
                        `[ERROR] media worker ${index} did not come back, retrying in ${delay / 1000}s: ${String(failure)}`,
                    );
                    setTimeout(() => attempt(Math.min(delay * 2, RESPAWN_CEILING_MS)), delay);
                });
        };

        attempt(1000);
    }

    /**
     * O router vai para o worker vivo com menos routers, fora dos que a sala já usa: dois
     * routers da mesma sala no mesmo worker dividiriam o mesmo núcleo e só somariam o custo
     * do pipe. A primeira vez (`busy` vazio) sem worker vivo é erro; a expansão sem worker
     * livre devolve `null`, e a sala fica onde está.
     */
    private async createRouter(busy: Worker[]): Promise<MediaRouter | null> {
        const alive = this.slots.filter((slot) => !slot.worker.closed);

        if (alive.length === 0) {
            throw new ServiceUnavailableException('no media worker is running');
        }

        const free = alive.filter((slot) => !busy.includes(slot.worker));

        if (free.length === 0) {
            return null;
        }

        const slot = free.reduce((smallest, candidate) =>
            candidate.routers.size < smallest.routers.size ? candidate : smallest,
        );
        const router = await slot.worker.createRouter({ mediaCodecs: config.router.mediaCodecs });

        slot.routers.add(router);
        router.observer.once('close', () => slot.routers.delete(router));

        return { router, webRtcServer: slot.webRtcServer, worker: slot.worker };
    }

    public find(roomId: string): Room | undefined {
        return this.rooms.get(roomId);
    }

    /**
     * Duas entradas na mesma sala nova chegam juntas com frequência (o link colado no
     * grupo). Sem a promessa compartilhada, cada uma criava o seu router: as duas pessoas
     * ficavam em salas diferentes com o mesmo código, e o primeiro router vazava.
     */
    public findOrCreate(roomId: string): Promise<Room> {
        const existing = this.rooms.get(roomId);

        if (existing) {
            return Promise.resolve(existing);
        }

        let pending = this.creating.get(roomId);

        if (!pending) {
            pending = this.create(roomId).finally(() => this.creating.delete(roomId));
            this.creating.set(roomId, pending);
        }

        return pending;
    }

    private async create(roomId: string): Promise<Room> {
        const room = await Room.create(roomId, (busy) => this.createRouter(busy));

        room.onEvicted = (empty) => this.release(empty);

        this.rooms.set(roomId, room);

        return room;
    }

    public replaceAccount(peer: Peer): void {
        if (peer.userId.startsWith('guest:')) {
            return;
        }

        for (const room of [...this.rooms.values()]) {
            for (const other of [...room.peers.values()]) {
                if (other !== peer && other.userId === peer.userId) {
                    room.replacePeer(other);
                }
            }
        }
    }

    public release(room: Room): void {
        if (this.rooms.get(room.id) !== room || room.activeCount() > 0 || !room.isEmpty()) {
            return;
        }

        room.close();
        this.rooms.delete(room.id);
    }

    /**
     * Quem está na carência de reconexão vem junto, com `reconnecting`: é o SFU quem sabe
     * quem ainda tem lugar na sala, e o Laravel decide por isso se o token é de reconexão.
     */
    public presence(): Record<
        string,
        {
            sub: string;
            name: string;
            sources: string[];
            muted: boolean;
            deafened: boolean;
            reconnecting: boolean;
        }[]
    > {
        return Object.fromEntries(
            [...this.rooms.values()].map((room) => [
                room.id,
                [...room.peers.values()].map((peer) => ({
                    sub: peer.userId,
                    name: peer.name,
                    sources: peer.sources(),
                    muted: peer.muted,
                    deafened: peer.deafened,
                    reconnecting: peer.isOrphaned(),
                })),
            ]),
        );
    }

    /** `workers` é quantos routers cada worker carrega: uma sala espalhada conta em cada um. */
    public stats(): {
        rooms: number;
        peers: number;
        workers: number[];
        workersDown: number;
    } {
        return {
            rooms: this.rooms.size,
            peers: [...this.rooms.values()].reduce((total, room) => total + room.peers.size, 0),
            workers: this.slots.map((slot) => slot.routers.size),
            workersDown: this.slots.filter((slot) => slot.worker.closed).length,
        };
    }

    /**
     * O que cada worker segura agora, contado pelo próprio mediasoup: os transportes (os de
     * pipe também), os producers e os consumers de cada router, a memória e a CPU do processo.
     */
    public async dump(): Promise<{
        rooms: number;
        peers: number;
        workers: WorkerDump[];
        node: { rssKb: number; heapUsedKb: number; cpuMs: number };
    }> {
        const workers = await Promise.all(
            this.slots.map(async (slot, index): Promise<WorkerDump> => {
                const empty = { transports: 0, producers: 0, consumers: 0 };

                if (slot.worker.closed) {
                    return {
                        index,
                        pid: slot.worker.pid,
                        closed: true,
                        routers: 0,
                        ...empty,
                        maxRssKb: 0,
                        cpuMs: 0,
                    };
                }

                // Router que fecha no meio da conta já não segura nada: fica fora dela. O worker
                // que morre no meio também, e o `/stats` responde com o que sobrou.
                const [usage, ...dumped] = await Promise.all([
                    slot.worker.getResourceUsage().catch(() => null),
                    ...[...slot.routers].map((router) => router.dump().catch(() => null)),
                ]);
                const routers = dumped.filter((router) => router !== null);

                return {
                    index,
                    pid: slot.worker.pid,
                    closed: false,
                    routers: routers.length,
                    ...routers.reduce(
                        (total, router) => ({
                            transports: total.transports + router.transportIds.length,
                            producers: total.producers + router.mapProducerIdConsumerIds.length,
                            consumers: total.consumers + router.mapConsumerIdProducerId.length,
                        }),
                        empty,
                    ),
                    maxRssKb: usage?.ru_maxrss ?? 0,
                    cpuMs: usage ? usage.ru_utime + usage.ru_stime : 0,
                };
            }),
        );
        const memory = process.memoryUsage();
        const cpu = process.cpuUsage();

        return {
            ...this.stats(),
            workers,
            node: {
                rssKb: Math.round(memory.rss / 1024),
                heapUsedKb: Math.round(memory.heapUsed / 1024),
                cpuMs: Math.round((cpu.user + cpu.system) / 1000),
            },
        };
    }
}
