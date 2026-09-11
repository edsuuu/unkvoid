import type {
    PlainTransport,
    Producer,
    Router,
    SrtpParameters,
    WebRtcServer,
    WebRtcTransport,
    Worker,
} from 'mediasoup/types';
import { randomUUID, randomBytes } from 'node:crypto';
import type { WebSocket } from 'ws';

import { config } from '../config.js';
import { NotFoundException, ValidationException } from '../Exceptions/ApiException.js';
import type { PeerDescription } from '../types.js';
import { Peer } from './Peer.js';

type ProducerOwner = { peer: Peer; producer: Producer };

export type JoinOutcome = { peer: Peer; resumed: boolean };

/**
 * Quanto tempo a sessão sobrevive sem sinalização. A mídia do WebRTC não cai junto com
 * o WebSocket, então segurar a pessoa aqui transforma uma queda de rede em engasgo em
 * vez de queda de chamada.
 */
const GRACE_MS = 30_000;

export class Room {
    public readonly peers = new Map<string, Peer>();

    /** Devolve a sala ao registro quando ela esvazia. O registro é quem ignora se não esvaziou. */
    public onEvicted: ((room: Room) => void) | null = null;

    private readonly evictions = new Map<string, NodeJS.Timeout>();

    public constructor(
        public readonly id: string,
        public readonly router: Router,
        private readonly webRtcServer: WebRtcServer,
    ) {}

    public static async create(
        worker: Worker,
        webRtcServer: WebRtcServer,
        id: string,
    ): Promise<Room> {
        const router = await worker.createRouter({ mediaCodecs: config.router.mediaCodecs });

        return new Room(id, router, webRtcServer);
    }

    /**
     * Três caminhos: retomar uma sessão órfã (mídia intacta), encerrar a sessão anterior
     * da mesma pessoa, ou criar uma do zero.
     *
     * Quem prova ser a mesma pessoa é a `resumeKey`, e não o `peerId`: o id a sala
     * inteira recebe no `peerJoined`, então aceitá-lo como identidade deixaria qualquer
     * um derrubar qualquer um só entrando com o id alheio.
     */
    public addPeer(
        name: string,
        socket: WebSocket,
        identity: { userId: string; owner: boolean },
        options: { resumeKey?: string | null; resume?: boolean } = {},
    ): JoinOutcome {
        const previous = options.resumeKey ? this.findByResumeKey(options.resumeKey) : null;

        if (previous?.isOrphaned() && options.resume) {
            this.cancelEviction(previous.id);
            previous.attachSocket(socket);
            this.broadcast('peerReconnected', { peerId: previous.id }, previous.id);

            return { peer: previous, resumed: true };
        }

        if (previous) {
            this.cancelEviction(previous.id);
            previous.send('replaced', { reason: 'you opened this room in another window' });
            this.peers.delete(previous.id);
            previous.close();
            previous.socket.close();
            this.broadcast('peerLeft', { peerId: previous.id }, previous.id);
        }

        const peer = new Peer(
            randomUUID(),
            name,
            socket,
            randomBytes(16).toString('hex'),
            identity.userId,
            identity.owner,
        );

        this.peers.set(peer.id, peer);

        return { peer, resumed: false };
    }

    /**
     * Expulsa todas as sessões de uma conta. Quem chama é o Laravel, depois de gravar o
     * banimento: a metade que impede a volta mora lá, porque é lá que o token nasce.
     */
    public kickUser(userId: string): number {
        let kicked = 0;

        for (const peer of [...this.peers.values()]) {
            if (peer.userId !== userId) {
                continue;
            }

            this.broadcast('peerKicked', { peerId: peer.id, name: peer.name }, peer.id);
            peer.send('kicked', { reason: 'você foi removido desta sala' });
            this.removePeer(peer);
            kicked += 1;
        }

        return kicked;
    }

    private findByResumeKey(resumeKey: string): Peer | null {
        // Varredura porque sala é coisa de dezenas, não de milhares: um índice a mais
        // seria outra estrutura para manter em sincronia com esta.
        return [...this.peers.values()].find((peer) => peer.resumeKey === resumeKey) ?? null;
    }

    /**
     * A sinalização caiu: segura a pessoa por GRACE_MS antes de destruir. A sala só é
     * avisada quando a carência expira de verdade.
     */
    public orphanPeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.orphanedAt = Date.now();

        // Avisa a sala na hora: sem isto, quem assistia ficava com o último quadro
        // congelado, sem saber que a conexão de quem transmitia tinha caído.
        this.broadcast('peerConnectionLost', { peerId: peer.id }, peer.id);

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

    /** Quem tem sinalização viva. Órfãos não contam na hora de fechar a sala. */
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

    /**
     * Recebe o objeto, não o id: fechar o socket de uma sessão substituída não pode
     * encerrar a sessão nova, que carrega o mesmo id de participante.
     */
    public removePeer(peer: Peer): void {
        if (this.peers.get(peer.id) !== peer) {
            return;
        }

        peer.close();
        this.peers.delete(peer.id);
        this.broadcast('peerLeft', { peerId: peer.id }, peer.id);

        // Sair de propósito também esvazia a sala. Sem isto, só a expiração da carência
        // devolvia o router ao registro, e uma sala de onde todo mundo saiu no botão
        // ficava alocada até o processo reiniciar.
        this.onEvicted?.(this);
    }

    public describePeers(exceptPeerId?: string): PeerDescription[] {
        return [...this.peers.values()]
            .filter((peer) => peer.id !== exceptPeerId && !peer.isOrphaned())
            .map((peer) => ({
                peerId: peer.id,
                userId: peer.userId,
                name: peer.name,
                producers: peer.describeProducers(),
            }));
    }

    public async createTransport(peer: Peer): Promise<WebRtcTransport> {
        const transport = await this.router.createWebRtcTransport({
            webRtcServer: this.webRtcServer,
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

        peer.addTransport(transport);

        return transport;
    }

    /**
     * Ingest de quem não é navegador: o app já codificou H.264 na GPU e manda RTP direto
     * nesta porta, sem ICE e sem DTLS.
     *
     * `comedia` faz o transport aprender o endereço do remetente no primeiro pacote, então
     * o app não precisa de porta alcançável — que é o ponto, já que ele vive atrás do
     * roteador de casa. SRTP não é opcional: sem ele a tela atravessaria a internet limpa.
     *
     * Uma transmissão, um transport: vídeo e áudio dividem. O mediasoup aceita vários
     * `produce()` no mesmo transport e o SSRC já distingue um do outro — um transport por
     * mídia gastaria o dobro de portas UDP, e cada porta a mais é uma linha a mais na
     * regra de firewall que alguém cria à mão.
     */
    public async plainTransportFor(
        peer: Peer,
        srtpParameters: SrtpParameters,
    ): Promise<PlainTransport> {
        return this.plainTransport(peer, srtpParameters, false);
    }

    /**
     * O transport por onde o app sem WebRTC RECEBE. É outro, e não o de transmitir, de
     * propósito: `comedia` aprende um endereço só por transport, e o de transmitir já
     * aponta para o socket que manda — o que chega teria de vir por ele.
     */
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

        if (existing) {
            return existing;
        }

        // `no more available ports` é o texto do mediasoup, e ele não diz nada a quem
        // só clicou em compartilhar. O limite é real: a faixa de portas do worker.
        const transport = await this.router
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
                appData: { receive },
            })
            .catch((failure) => {
                throw /no more available ports/i.test(String(failure))
                    ? new ValidationException(
                          `o servidor já está no limite de ${config.plainPortsPerWorker} transmissões ao mesmo tempo — peça para alguém parar de compartilhar`,
                      )
                    : failure;
            });

        await transport.connect({ srtpParameters });

        peer.addPlainTransport(transport);

        return transport;
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

    public close(): void {
        for (const timer of this.evictions.values()) {
            clearTimeout(timer);
        }

        this.evictions.clear();
        this.router.close();
        this.peers.clear();
    }
}
