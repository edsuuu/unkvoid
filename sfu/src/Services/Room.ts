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
import type { SourceName } from '../Enums/Source.js';
import { NotFoundException, ValidationException } from '../Exceptions/ApiException.js';
import type { PeerDescription } from '../types.js';
import { Peer } from './Peer.js';
import { Webhook } from './Webhook.js';

export type ProducerOwner = { peer: Peer; producer: Producer };

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
     * Três caminhos: retomar a sessão (mídia intacta), encerrar a sessão anterior da mesma
     * pessoa, ou criar uma do zero.
     *
     * A retomada não espera a sessão ficar órfã. O app mede o socket de 5 em 5 s e percebe
     * a queda antes do heartbeat daqui; exigir a carência aberta fazia essa volta virar
     * entrada nova, que derruba a antiga e a mídia com ela.
     *
     * Quem prova ser a mesma pessoa é a `resumeKey`, e não o `peerId`: o id a sala
     * inteira recebe no `peerJoined`, então aceitá-lo como identidade deixaria qualquer
     * um derrubar qualquer um só entrando com o id alheio.
     */
    public addPeer(
        name: string,
        socket: WebSocket,
        identity: { userId: string; can: string[]; ip: string },
        options: { resumeKey?: string | null; resume?: boolean } = {},
    ): JoinOutcome {
        const previous = options.resumeKey ? this.findByResumeKey(options.resumeKey) : null;

        // Visitante não traz token: o `can` dele é sempre o cheio, e o `sub` é sorteado a
        // cada entrada quando o app não manda `installId`. Só o token tem conta a conferir.
        const tokened = !identity.userId.startsWith('guest:');

        // Token de outra conta não retoma: daria a esta sessão o `can` que o Laravel
        // assinou para outra pessoa. Vira entrada nova, que substitui a antiga.
        const stranger = tokened && previous?.userId !== identity.userId;

        if (previous && options.resume && !stranger) {
            const staleSocket = previous.isOrphaned() ? null : previous.socket;

            this.cancelEviction(previous.id);
            previous.attachSocket(socket);

            // Só volta quem a sala viu cair: na troca com a sessão de pé ninguém recebeu
            // `peerConnectionLost`.
            if (!staleSocket) {
                this.broadcast('peerReconnected', { peerId: previous.id }, previous.id);
            }

            if (tokened) {
                this.applyCan(previous, identity.can);
            }

            // Quem voltou nunca parou de receber a mídia — a carência existe para isso.
            // Sem reanunciar, quem transmite ficaria vendo "ninguém assistindo" pelo resto
            // da transmissão, porque a queda tirou essa pessoa da plateia.
            for (const producerId of new Set(
                [...previous.consumers.values()].map((consumer) => consumer.producerId),
            )) {
                this.announceWatchers(producerId);
            }

            if (staleSocket) {
                console.log(
                    `[INFO] socket swapped room=${this.id} sub=${previous.userId} peer=${previous.id} ip=${previous.ip}`,
                );
                // `terminate`, e não `close`: o TCP deste socket já sumiu, e o aperto de mão
                // do fechamento ficaria 30 s esperando uma resposta que não vem. Sem
                // `replaced` também: a pessoa não entrou de outro lugar, é ela mesma voltando.
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

        this.peers.set(peer.id, peer);

        // Depois de a nova estar na sala: tirar a antiga antes deixaria a sala vazia por
        // um instante, e o registro fecharia o router debaixo de quem acabou de entrar.
        if (previous) {
            this.replacePeer(previous);
        }

        return { peer, resumed: false };
    }

    /**
     * Encerra uma sessão que outra, da mesma pessoa, veio substituir. Sem isto a antiga
     * seguia de pé ao lado da nova: a mesma conta duas vezes na lista, e o app antigo
     * disputando a porta de RTP puro com o novo.
     */
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
            // Sem fechar o socket a sessão expulsa seguia viva e alocando transports.
            peer.socket.close(4001, 'kicked');
            this.removePeer(peer);
            kicked += 1;
        }

        return kicked;
    }

    /** Silencia (ou devolve a voz a) todas as sessões de uma conta. Também vem do Laravel. */
    public async muteUser(userId: string, muted: boolean): Promise<number> {
        let touched = 0;

        for (const peer of this.peers.values()) {
            if (peer.userId !== userId) {
                continue;
            }

            peer.serverMuted = muted;
            peer.send('serverMuted', { muted });

            for (const producer of peer.producers.values()) {
                if (producer.appData.source === 'mic') {
                    await this.setProducerPaused(peer, producer, muted);
                    touched += 1;
                }
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

    /**
     * Quem decide permissão é o Laravel, e o token da reconexão é a palavra mais recente
     * dele. O que o `can` novo não cobre sai do ar agora: esperar a próxima entrada deixaria
     * a tela de quem perdeu `stream` na carência no ar até a pessoa sair por conta própria.
     */
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
            // `producerClosed` fala com a sala inteira MENOS o dono: sem este aviso o app
            // dele seguia "ao vivo", codificando para um transporte que já não existe.
            //
            // Só tela. O app já instalado derruba a transmissão com QUALQUER `producerDead`,
            // sem olhar a origem: avisar do mic ou da câmera tiraria do ar uma tela que
            // continua permitida. Desses dois o app fica sabendo pelo `can` da resposta.
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

        // Só o transport de RTP puro, e quando sai o último producer que passa por ele. O mic
        // por WebRTC do Windows e do macOS não conta: com ele o transport ficava vivo, preso ao
        // socket antigo do app, que abre outro na transmissão seguinte e some no `comedia`.
        if (![...peer.producers.values()].some((other) => other.appData.plain === true)) {
            peer.closePlainTransports();
        }
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

        // Quem caiu sai da plateia agora, e não daqui a trinta segundos quando a carência
        // estourar: o quadro dela congelou no instante em que a conexão foi embora.
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

        console.log(`[INFO] left room=${this.id} sub=${peer.userId} peer=${peer.id} ip=${peer.ip}`);
        peer.close();
        this.peers.delete(peer.id);
        this.broadcast('peerLeft', { peerId: peer.id }, peer.id);

        // A conta continua no canal enquanto outra sessão dela estiver aqui (mesmo em
        // carência: ou ela volta, ou avisa por conta própria quando expirar).
        if (![...this.peers.values()].some((other) => other.userId === peer.userId)) {
            Webhook.send('left', this.id, peer);
        }

        // Sair de propósito também esvazia a sala. Sem isto, só a expiração da carência
        // devolvia o router ao registro, e uma sala de onde todo mundo saiu no botão
        // ficava alocada até o processo reiniciar.
        this.onEvicted?.(this);
    }

    public describePeers(exceptPeerId?: string, withOrphans = false): PeerDescription[] {
        return [...this.peers.values()]
            .filter((peer) => peer.id !== exceptPeerId && (withOrphans || !peer.isOrphaned()))
            .map((peer) => ({
                peerId: peer.id,
                userId: peer.userId,
                name: peer.name,
                reconnecting: peer.isOrphaned(),
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
                          'o servidor já está no limite de participantes por sala — tente de novo quando alguém sair',
                      )
                    : failure;
            });

        await transport.connect({ srtpParameters });

        peer.addPlainTransport(transport);

        return transport;
    }

    /**
     * Quem está recebendo esta transmissão agora, para a sala inteira: quem compartilha
     * quer ver a plateia, e quem assiste quer saber que não está sozinho.
     *
     * **Só tela.** Microfone é todo mundo consumindo todo mundo, e câmera também: numa
     * sala de oito com a câmera ligada, anunciar por consumer daria centenas de mensagens
     * a cada pessoa que entra. Tela é a única em que existe plateia de verdade.
     *
     * Plateia é quem está **olhando**, não quem tem o consumer: cartão pausado, escondido
     * ou em segundo plano pausa o consumer, e aí a pessoa sai da lista. Quem caiu e está na
     * carência também sai — o quadro dela já congelou.
     *
     * ponytail: varre todos os peers vezes os consumers de cada um, a cada retomada ou
     * pausa. Numa sala de dezenas é ruído; se um dia existir sala de centenas, o caminho é
     * um índice `producerId -> peers` mantido no `track()` do ConsumerController.
     */
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

    public close(): void {
        for (const timer of this.evictions.values()) {
            clearTimeout(timer);
        }

        this.evictions.clear();
        this.router.close();
        this.peers.clear();
    }
}
