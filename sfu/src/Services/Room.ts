import type {
    PlainTransport,
    Producer,
    Router,
    RtpCapabilities,
    SrtpParameters,
    WebRtcServer,
    WebRtcTransport,
    Worker,
} from 'mediasoup/types';
import { randomUUID, randomBytes } from 'node:crypto';
import type { WebSocket } from 'ws';

import { Peer, type ProducerDescription } from './Peer.js';
import { Webhook } from './Webhook.js';
import { config } from '../Config/index.js';
import type { SourceName } from '../Enums/Source.js';
import {
    NotFoundException,
    ServiceUnavailableException,
    ValidationException,
} from '../Exceptions/ApiException.js';

export type ProducerOwner = { peer: Peer; producer: Producer };

export type JoinOutcome = { peer: Peer; resumed: boolean };

const GRACE_MS = 30_000;

const MAX_TRANSPORTS = 8;

export type PeerDescription = {
    peerId: string;
    userId: string;
    name: string;
    reconnecting: boolean;
    muted: boolean;
    deafened: boolean;
    producers: ProducerDescription[];
};

export type Move = { to: string; by: string | null };

export type MediaRouter = { router: Router; webRtcServer: WebRtcServer; worker: Worker };

/** Pede ao registro um router num worker que a sala ainda não usa; `null` quando não há. */
export type RouterSource = (busy: Worker[]) => Promise<MediaRouter | null>;

export class Room {
    public readonly peers = new Map<string, Peer>();

    public onEvicted: ((room: Room) => void) | null = null;

    /**
     * Quando a primeira pessoa entrou: a sala nasce com ela e some com a última. É daqui que
     * o relógio da barra conta, igual para todo mundo, como a duração de uma chamada.
     */
    public readonly createdAt = Date.now();

    private readonly evictions = new Map<string, NodeJS.Timeout>();

    private readonly routers: MediaRouter[];

    private readonly piped = new Map<string, Promise<void>>();

    private expanding: Promise<MediaRouter | null> | null = null;

    public constructor(
        public readonly id: string,
        first: MediaRouter,
        private readonly source: RouterSource,
    ) {
        this.routers = [first];
    }

    public static async create(id: string, source: RouterSource): Promise<Room> {
        const first = await source([]);

        if (!first) {
            throw new ServiceUnavailableException('no media worker is running');
        }

        return new Room(id, first, source);
    }

    /** Os routers da sala nascem dos mesmos codecs, então as capacidades são as mesmas em todos. */
    public rtpCapabilities(): RtpCapabilities {
        return this.routers[0]!.router.rtpCapabilities;
    }

    public uses(worker: Worker): boolean {
        return this.routers.some((media) => media.worker === worker);
    }

    /**
     * Uma sala num worker só é uma sala num núcleo só: com 25 pessoas e câmeras são mais de mil
     * consumers, e o núcleo satura para todo mundo junto. Quem chega depois que o router encheu
     * vai para um router novo noutro worker, e o que ele assiste de lá chega pelo
     * `pipeToRouter`. A pessoa só ganha router quando abre o primeiro transporte: quem entra
     * só para o chat não ocupa lugar.
     *
     * ponytail: "cheio" é contagem de pessoas (`SFU_PEERS_PER_ROUTER`), não carga medida do
     * worker. Se a conta errar, medir com `worker.getResourceUsage()` ou contar consumers.
     */
    public async routerOf(peer: Peer): Promise<MediaRouter> {
        peer.routing ??= this.pickRouter().then(
            (media) => (peer.media = media),
            (failure: unknown) => {
                peer.routing = null;
                throw failure;
            },
        );

        return peer.routing;
    }

    private async pickRouter(): Promise<MediaRouter> {
        const load = (media: MediaRouter): number =>
            [...this.peers.values()].filter((peer) => peer.media === media).length;
        const roomy = this.routers.find((media) => load(media) < config.peersPerRouter);

        if (roomy) {
            return roomy;
        }

        this.expanding ??= this.source(this.routers.map((media) => media.worker))
            .then((fresh) => {
                if (fresh) {
                    this.routers.push(fresh);
                    console.log(
                        `[INFO] room=${this.id} spread to ${this.routers.length} media workers`,
                    );
                }

                return fresh;
            })
            .finally(() => (this.expanding = null));

        const fresh = await this.expanding;

        if (fresh) {
            return fresh;
        }

        return this.routers.reduce((smallest, media) =>
            load(media) < load(smallest) ? media : smallest,
        );
    }

    /**
     * O producer mora no router de quem produz. Para alguém de outro router consumir, ele
     * é espelhado lá uma vez só (o mediasoup recusa o mesmo id duas vezes no mesmo router),
     * e o espelho fecha, pausa e retoma junto com o original.
     */
    public async pipe(producerId: string, target: MediaRouter): Promise<void> {
        const source = this.findProducerOwner(producerId).peer.media;

        if (!source || source === target) {
            return;
        }

        const key = `${producerId}:${target.router.id}`;
        let piping = this.piped.get(key);

        if (!piping) {
            piping = source.router
                .pipeToRouter({ producerId, router: target.router, enableSctp: false })
                .then(({ pipeProducer }) => {
                    pipeProducer?.observer.once('close', () => this.piped.delete(key));
                });
            piping.catch(() => this.piped.delete(key));
            this.piped.set(key, piping);
        }

        await piping;
    }

    public addPeer(
        name: string,
        socket: WebSocket,
        identity: { userId: string; can: string[]; muted?: boolean; ip: string },
        options: { resumeKey?: string | null; resume?: boolean } = {},
    ): JoinOutcome {
        const previous = options.resumeKey ? this.findByResumeKey(options.resumeKey) : null;

        const tokened = !identity.userId.startsWith('guest:');

        const stranger = tokened && previous?.userId !== identity.userId;

        if (previous && options.resume && !stranger) {
            const staleSocket = previous.isOrphaned() ? null : previous.socket;

            this.cancelEviction(previous.id);
            previous.attachSocket(socket);

            if (!staleSocket) {
                this.broadcast('peerReconnected', { peerId: previous.id }, previous.id);
            }

            if (tokened) {
                this.applyCan(previous, identity.can);

                // O mudo do servidor vem no token da retomada como vinha no `can`: um `/mute`
                // perdido se acerta na primeira oscilação.
                if (previous.serverMuted !== (identity.muted === true)) {
                    // Sem o catch, um producer ou worker já fechado derrubava o processo inteiro.
                    this.applyServerMute(previous, identity.muted === true).catch(
                        (failure: unknown) => {
                            console.warn(
                                `[WARN] serverMute room=${this.id} sub=${previous.userId} peer=${previous.id}: ${String(failure)}`,
                            );
                        },
                    );
                }
            }

            for (const producerId of new Set(
                [...previous.consumers.values()].map((consumer) => consumer.producerId),
            )) {
                this.announceWatchers(producerId);
            }

            if (staleSocket) {
                console.log(
                    `[INFO] socket swapped room=${this.id} sub=${previous.userId} peer=${previous.id} ip=${previous.ip}`,
                );

                staleSocket.terminate();
            }

            return { peer: previous, resumed: true };
        }

        const peer = new Peer(
            randomUUID(),
            name,
            socket,
            randomBytes(16).toString('hex'),
            identity.userId,
            identity.can,
            identity.ip,
        );

        // Mutado pelo servidor já entra calado: o token leva `speak` (a permissão) e a marca
        // separada, para o desmutar devolver a voz sem a pessoa sair e entrar.
        peer.serverMuted = identity.muted === true;

        this.peers.set(peer.id, peer);

        if (previous) {
            this.replacePeer(previous);
        }

        return { peer, resumed: false };
    }

    public replacePeer(previous: Peer): void {
        console.log(
            `[INFO] replaced room=${this.id} sub=${previous.userId} peer=${previous.id} ip=${previous.ip}`,
        );
        this.cancelEviction(previous.id);
        previous.send('replaced', { reason: 'you joined again from another connection' });
        previous.socket.close(4002, 'replaced');
        this.removePeer(previous);
    }

    /**
     * Mover é expulsar daqui com destino: quem foi movido recebe `moved` e entra sozinho no
     * outro canal, e a sala vê só o `peerLeft` de sempre, porque ninguém foi punido.
     */
    public kickUser(userId: string, move: Move | null = null): number {
        let kicked = 0;

        for (const peer of [...this.peers.values()]) {
            if (peer.userId !== userId) {
                continue;
            }

            if (move) {
                console.log(
                    `[INFO] moved room=${this.id} sub=${peer.userId} to=${move.to} by=${JSON.stringify(move.by)} peer=${peer.id} ip=${peer.ip}`,
                );
                peer.send('moved', move);
                peer.socket.close(4003, 'moved');
            } else {
                this.broadcast('peerKicked', { peerId: peer.id, name: peer.name }, peer.id);
                peer.send('kicked', { reason: 'você foi removido desta sala' });
                peer.socket.close(4001, 'kicked');
            }

            this.removePeer(peer);
            kicked += 1;
        }

        return kicked;
    }

    public async muteUser(userId: string, muted: boolean): Promise<number> {
        let touched = 0;

        for (const peer of this.peers.values()) {
            if (peer.userId !== userId) {
                continue;
            }

            touched += await this.applyServerMute(peer, muted);
        }

        return touched;
    }

    /**
     * A marca do mudo do servidor, e o mic junto com ela: pausado enquanto durar, retomado
     * quando o Laravel devolve a voz. Devolve quantos mics mexeu.
     */
    public async applyServerMute(peer: Peer, muted: boolean): Promise<number> {
        let touched = 0;

        peer.serverMuted = muted;
        peer.send('serverMuted', { muted });

        for (const producer of peer.producers.values()) {
            if (producer.appData.source === 'mic') {
                await this.setProducerPaused(peer, producer, muted);
                touched += 1;
            }
        }

        return touched;
    }

    public async setProducerPaused(peer: Peer, producer: Producer, paused: boolean): Promise<void> {
        if (!paused) {
            peer.assertNotServerMuted(String(producer.appData.source));
        }

        await (paused ? producer.pause() : producer.resume());

        this.broadcast(
            paused ? 'producerPaused' : 'producerResumed',
            { peerId: peer.id, producerId: producer.id },
            peer.id,
        );
    }

    private applyCan(peer: Peer, can: string[]): void {
        peer.can = can;

        for (const producer of [...peer.producers.values()]) {
            const source = String(producer.appData.source) as SourceName;

            if (peer.allows(source)) {
                continue;
            }

            console.log(
                `[INFO] revoked room=${this.id} sub=${peer.userId} source=${source} peer=${peer.id} ip=${peer.ip}`,
            );

            if (source === 'screen' || source === 'screenAudio') {
                peer.send('producerDead', {
                    producerId: producer.id,
                    kind: producer.kind,
                    source,
                    reason: 'revoked',
                });
            }

            this.closeProducer(peer, producer);
        }
    }

    public closeProducer(peer: Peer, producer: Producer | undefined): void {
        if (!producer || peer.producers.get(producer.id) !== producer) {
            return;
        }

        producer.close();
        peer.producers.delete(producer.id);
        this.broadcast(
            'producerClosed',
            {
                peerId: peer.id,
                producerId: producer.id,
                kind: producer.kind,
                source: String(producer.appData.source),
            },
            peer.id,
        );

        if (![...peer.producers.values()].some((other) => other.appData.plain === true)) {
            peer.closePlainTransports();
        }
    }

    private findByResumeKey(resumeKey: string): Peer | null {
        return [...this.peers.values()].find((peer) => peer.resumeKey === resumeKey) ?? null;
    }

    public orphanPeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.orphanedAt = Date.now();

        this.broadcast('peerConnectionLost', { peerId: peer.id }, peer.id);

        for (const producerId of new Set(
            [...peer.consumers.values()].map((consumer) => consumer.producerId),
        )) {
            this.announceWatchers(producerId);
        }

        this.evictions.set(
            peer.id,
            setTimeout(() => {
                this.evictions.delete(peer.id);

                if (this.peers.get(peer.id) === peer && peer.isOrphaned()) {
                    this.removePeer(peer);
                }
            }, GRACE_MS),
        );
    }

    private cancelEviction(peerId: string): void {
        const timer = this.evictions.get(peerId);

        if (timer) {
            clearTimeout(timer);
            this.evictions.delete(peerId);
        }
    }

    public activeCount(): number {
        return [...this.peers.values()].filter((peer) => !peer.isOrphaned()).length;
    }

    public findPeer(peerId: string): Peer {
        const peer = this.peers.get(peerId);

        if (!peer) {
            throw new NotFoundException(`participant ${peerId} is not in this room`);
        }

        return peer;
    }

    public removePeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        console.log(`[INFO] left room=${this.id} sub=${peer.userId} peer=${peer.id} ip=${peer.ip}`);
        peer.close();
        this.peers.delete(peer.id);
        this.shrink();
        this.broadcast('peerLeft', { peerId: peer.id }, peer.id);

        if (![...this.peers.values()].some((other) => other.userId === peer.userId)) {
            Webhook.send('left', this.id, peer);
        }

        this.onEvicted?.(this);
    }

    /**
     * O router que ficou sem ninguém sai da sala. Antes ele ficava até a sala acabar, e o
     * `pipeToRouter` seguia mandando cada pacote de cada tela para um router vazio, noutro
     * núcleo: a sala que encheu uma vez pagava o espalhamento para sempre. Fechar o router
     * fecha o par de pipes nos dois lados. Fica sempre um, e nada fecha enquanto alguém ainda
     * escolhe o seu (`routing` sem `media`) ou a sala está abrindo outro.
     */
    private shrink(): void {
        if (
            this.expanding ||
            [...this.peers.values()].some((peer) => peer.routing && !peer.media)
        ) {
            return;
        }

        for (const media of [...this.routers]) {
            if (
                this.routers.length < 2 ||
                [...this.peers.values()].some((peer) => peer.media === media)
            ) {
                continue;
            }

            this.routers.splice(this.routers.indexOf(media), 1);
            media.router.close();
            console.log(`[INFO] room=${this.id} back to ${this.routers.length} media workers`);
        }
    }

    public describePeers(exceptPeerId?: string, withOrphans = false): PeerDescription[] {
        return [...this.peers.values()]
            .filter((peer) => peer.id !== exceptPeerId && (withOrphans || !peer.isOrphaned()))
            .map((peer) => ({
                peerId: peer.id,
                userId: peer.userId,
                name: peer.name,
                reconnecting: peer.isOrphaned(),
                muted: peer.muted,
                deafened: peer.deafened,
                producers: peer.describeProducers(),
            }));
    }

    public async createTransport(peer: Peer): Promise<WebRtcTransport> {
        // O app abre dois (receber e enviar). Sem teto, um visitante de sala por código
        // pedia transporte em laço até o worker da sala morrer sem memória.
        if (peer.transports.size >= MAX_TRANSPORTS) {
            throw new ValidationException('too many open transports for this participant');
        }

        const media = await this.routerOf(peer);
        const transport = await media.router.createWebRtcTransport({
            webRtcServer: media.webRtcServer,
            enableUdp: config.transport.enableUdp,
            enableTcp: config.transport.enableTcp,
            preferUdp: config.transport.preferUdp,
            initialAvailableOutgoingBitrate: config.transport.initialAvailableOutgoingBitrate,
        });

        await transport.setMaxIncomingBitrate(config.transport.maxIncomingBitrate);

        transport.on('dtlsstatechange', (state) => {
            if (state === 'closed') {
                transport.close();
            }
        });

        transport.observer.once('close', () => peer.transports.delete(transport.id));

        peer.addTransport(transport);

        return transport;
    }

    public async plainTransportFor(
        peer: Peer,
        srtpParameters: SrtpParameters,
    ): Promise<PlainTransport> {
        return this.plainTransport(peer, srtpParameters, false);
    }

    public async plainReceiveTransportFor(
        peer: Peer,
        srtpParameters: SrtpParameters,
    ): Promise<PlainTransport> {
        return this.plainTransport(peer, srtpParameters, true);
    }

    private async plainTransport(
        peer: Peer,
        srtpParameters: SrtpParameters,
        receive: boolean,
    ): Promise<PlainTransport> {
        const existing = [...peer.plainTransports.values()].find(
            (transport) => Boolean(transport.appData.receive) === receive,
        );

        if (existing?.appData.key === srtpParameters.keyBase64) {
            return existing;
        }

        // Chave nova é o app refazendo o caminho: o `comedia` prendeu o transporte ao primeiro
        // endereço, e se o roteador da pessoa trocou de endereço tudo o que vem dele é
        // descartado. Fechar leva junto o que estava nele.
        if (existing) {
            existing.close();
            peer.plainTransports.delete(existing.id);
        }

        const media = await this.routerOf(peer);
        const transport = await media.router
            .createPlainTransport({
                listenInfo: {
                    protocol: 'udp',
                    ip: '0.0.0.0',
                    announcedAddress: config.announcedAddress,
                },
                rtcpMux: true,
                comedia: true,
                enableSrtp: true,
                srtpCryptoSuite: srtpParameters.cryptoSuite,
                appData: { receive, key: srtpParameters.keyBase64 },
            })
            .catch((failure) => {
                throw /no more available ports/i.test(String(failure))
                    ? new ValidationException(
                          'o servidor já está no limite de participantes por sala — tente de novo quando alguém sair',
                      )
                    : failure;
            });

        await transport.connect({ srtpParameters });

        peer.addPlainTransport(transport);

        return transport;
    }

    public announceWatchers(producerId: string): void {
        const owner = [...this.peers.values()].find((peer) => peer.producers.has(producerId));

        if (String(owner?.producers.get(producerId)?.appData.source) !== 'screen') {
            return;
        }

        const watchers = [...this.peers.values()]
            .filter(
                (peer) =>
                    !peer.isOrphaned() &&
                    [...peer.consumers.values()].some(
                        (consumer) => consumer.producerId === producerId && !consumer.paused,
                    ),
            )
            .map((peer) => ({ peerId: peer.id, name: peer.name }));

        this.broadcast('watchers', { producerId, watchers });
    }

    public findProducerOwner(producerId: string): ProducerOwner {
        for (const peer of this.peers.values()) {
            const producer = peer.producers.get(producerId);

            if (producer) {
                return { peer, producer };
            }
        }

        throw new NotFoundException(`producer ${producerId} does not exist in this room`);
    }

    public broadcast(event: string, data: unknown, exceptPeerId?: string): void {
        for (const peer of this.peers.values()) {
            if (peer.id !== exceptPeerId) {
                peer.send(event, data);
            }
        }
    }

    public isEmpty(): boolean {
        return this.peers.size === 0;
    }

    /**
     * Um worker da sala morreu e a mídia de quem estava nele foi junto. O 1012 (servidor
     * reiniciando) não é o 4001 nem o 4002: o app trata como queda, reconecta e publica de
     * novo, num router vivo. Quem está nos outros workers fica, e recebe o `producerClosed`
     * e o `consumerClosed` do que se perdeu. Fechar todos os sockets antes de tirar o
     * primeiro poupa os outros de uma rajada de `peerLeft` que eles não vão usar.
     */
    public evacuate(worker: Worker): void {
        const survivors = this.routers.filter((media) => media.worker !== worker);
        const lost = [...this.peers.values()].filter(
            (peer) => survivors.length === 0 || peer.media?.worker === worker,
        );

        this.routers.splice(0, this.routers.length, ...survivors);

        for (const peer of lost) {
            peer.socket.close(1012, 'media server restarted');
        }

        for (const peer of lost) {
            this.removePeer(peer);
        }
    }

    public close(): void {
        for (const timer of this.evictions.values()) {
            clearTimeout(timer);
        }

        this.evictions.clear();

        for (const media of this.routers) {
            media.router.close();
        }

        this.peers.clear();
    }
}
