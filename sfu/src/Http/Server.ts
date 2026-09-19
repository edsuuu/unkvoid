import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { WebSocketServer, type RawData, type WebSocket } from 'ws';

import { config } from '../config.js';
import { NotFoundException, ValidationException } from '../Exceptions/ApiException.js';
import { RoomRegistry } from '../Services/RoomRegistry.js';
import { Signature } from '../Services/Signature.js';
import type { Session } from '../types.js';
import { Kernel } from './Kernel.js';

type Payload = { id?: number; action?: string; data?: Record<string, unknown> };

const WINDOW_MS = 60_000;

/** Um corpo maior que isto não é uma chamada do Laravel. */
const MAX_BODY_BYTES = 16 * 1024;

const ROOM_ACTION_PATH = /^\/rooms\/([a-z0-9-]+)\/(kick|mute)$/;

/**
 * Um socket meio aberto — tampa do notebook fechada, Wi-Fi trocado por 4G — nunca manda
 * FIN nem RST. Sem perguntar de tempos em tempos se ele continua vivo, o `close` não
 * dispara, a pessoa fica eternamente ativa na sala, `activeCount()` nunca zera e o
 * router do mediasoup nunca é devolvido. As portas de RTP puro que ela segurava também
 * não voltam. É o vazamento que acaba batendo no `max_memory_restart` do pm2 e
 * derrubando a chamada de todo mundo.
 */

export class Server {
    private readonly registry = new RoomRegistry();

    private readonly kernel = new Kernel(this.registry);

    private readonly sessions = new Map<WebSocket, Session>();

    private readonly recent = new Map<string, number[]>();

    /** Quem respondeu ao último ping. Quem não respondeu perde a conexão no próximo. */
    private readonly alive = new WeakSet<WebSocket>();

    public async start(): Promise<void> {
        await this.registry.boot();

        const http = createServer((request, response) => void this.serve(request, response));

        const websockets = new WebSocketServer({ server: http, path: config.path });

        websockets.on('connection', (socket, request) => this.accept(socket, request));

        setInterval(() => {
            for (const socket of websockets.clients) {
                if (!this.alive.has(socket)) {
                    // `terminate` fecha na marra e dispara o `close`, que é o que põe a
                    // carência de 30 segundos para andar.
                    socket.terminate();

                    continue;
                }

                this.alive.delete(socket);
                socket.ping();
            }
        }, config.heartbeatMs).unref();

        http.listen(config.listenPort, config.listenHost, () =>
            console.log(
                `[INFO] SFU em ${config.listenHost}:${config.listenPort}${config.path} · media on port ${config.mediaPort}`,
            ),
        );
    }

    /**
     * O pouco de HTTP que existe: o `/health` que o app consulta antes de entrar, e a
     * porta pela qual o Laravel manda expulsar ou silenciar alguém e pergunta quem está
     * em cada sala. Tudo o mais é WebSocket.
     */
    private async serve(request: IncomingMessage, response: ServerResponse): Promise<void> {
        const path = (request.url ?? '').split('?')[0] ?? '';

        try {
            if (request.method === 'GET' && path === '/health') {
                this.reply(response, 200, {
                    ok: true,
                    appVersion: config.appVersion,
                    ...this.registry.stats(),
                });

                return;
            }

            const roomAction = ROOM_ACTION_PATH.exec(path);

            if (request.method === 'POST' && roomAction?.[1]) {
                const body = await this.body(request);

                this.verifySignature(request, path, body);

                const { userId, muted } = JSON.parse(body) as { userId?: unknown; muted?: unknown };

                if (typeof userId !== 'string' || userId === '') {
                    throw new ValidationException('field userId is required');
                }

                // Sala que não está no ar não tem quem expulsar: o banimento já foi gravado
                // do outro lado, e é ele que impede a volta.
                const room = this.registry.find(roomAction[1]);

                if (roomAction[2] === 'kick') {
                    this.reply(response, 200, { kicked: room?.kickUser(userId) ?? 0 });

                    return;
                }

                if (typeof muted !== 'boolean') {
                    throw new ValidationException('field muted must be true or false');
                }

                this.reply(response, 200, { muted: (await room?.muteUser(userId, muted)) ?? 0 });

                return;
            }

            if (request.method === 'GET' && path === '/presence') {
                this.verifySignature(request, path, '');
                this.reply(response, 200, { rooms: this.registry.presence() });

                return;
            }

            throw new NotFoundException();
        } catch (exception) {
            // Corpo que não é JSON estoura `SyntaxError` no `JSON.parse`: é erro de quem
            // chamou, não do servidor.
            const status = exception instanceof SyntaxError ? 422 : Kernel.statusOf(exception);
            const message = exception instanceof Error ? exception.message : 'unexpected error';

            if (status === 500) {
                console.error(`[ERROR] http ${request.method} ${path}: ${message}`);
            }

            this.reply(response, status, { ok: false, error: message });
        }
    }

    /**
     * Assinado pelo Laravel com o mesmo segredo do token. Sem isto qualquer um que
     * alcançasse a porta 3000 expulsaria quem quisesse.
     */
    private verifySignature(request: IncomingMessage, path: string, body: string): void {
        Signature.verifyHeader(
            request.headers['x-unkvoid-timestamp']?.toString(),
            request.headers['x-unkvoid-signature']?.toString(),
            request.method ?? '',
            path,
            body,
        );
    }

    private body(request: IncomingMessage): Promise<string> {
        return new Promise((resolve, reject) => {
            const chunks: Buffer[] = [];
            let size = 0;

            request.on('data', (chunk: Buffer) => {
                size += chunk.length;

                if (size > MAX_BODY_BYTES) {
                    request.destroy();
                    reject(new ValidationException('body too large'));

                    return;
                }

                chunks.push(chunk);
            });
            request.on('end', () => resolve(Buffer.concat(chunks).toString()));
            request.on('error', reject);
        });
    }

    private reply(response: ServerResponse, status: number, data: Record<string, unknown>): void {
        response.writeHead(status, { 'content-type': 'application/json' });
        response.end(JSON.stringify(data));
    }

    private accept(socket: WebSocket, request: IncomingMessage): void {
        const ip = addressOf(request);

        if (this.tooMany(ip)) {
            socket.close(1013, 'too many connections — try again in a minute');

            return;
        }

        const session: Session = { socket, ip, room: null, peer: null };

        this.sessions.set(socket, session);

        this.alive.add(socket);
        socket.on('pong', () => this.alive.add(socket));
        socket.on('message', (raw) => void this.handle(session, raw));
        socket.on('close', () => this.release(session));
        socket.on('error', (error) => console.error('[ERROR] socket', error.message));
    }

    private tooMany(address: string): boolean {
        const now = Date.now();
        const hits = (this.recent.get(address) ?? []).filter((at) => now - at < WINDOW_MS);
        const over = hits.length >= config.connectionsPerMinute;

        // Quem já passou do teto não entra na conta: senão cada tentativa recusada
        // empurrava a janela para a frente e o bloqueio nunca expirava.
        if (!over) {
            hits.push(now);
        }

        this.recent.set(address, hits);

        // ponytail: varre o mapa inteiro quando ele cresce. Um LRU só valeria a pena na
        // ordem de milhares de IPs por minuto, que não é o tamanho disto.
        if (this.recent.size > 1000) {
            for (const [known, times] of this.recent) {
                if (times.every((at) => now - at >= WINDOW_MS)) {
                    this.recent.delete(known);
                }
            }
        }

        return over;
    }

    private async handle(session: Session, raw: RawData): Promise<void> {
        let payload: Payload;

        try {
            payload = JSON.parse(raw.toString()) as Payload;
        } catch {
            console.error('[WARN] discarded invalid message');

            return;
        }

        try {
            const data = await this.kernel.dispatch(payload.action ?? '', payload.data, session);

            session.socket.send(JSON.stringify({ id: payload.id, ok: true, data }));
        } catch (exception) {
            const status = Kernel.statusOf(exception);
            const message = exception instanceof Error ? exception.message : 'unexpected error';

            console.error(`[ERROR] ${payload.action} (${status}): ${message}`);
            session.socket.send(
                JSON.stringify({ id: payload.id, ok: false, status, error: message }),
            );
        }
    }

    private release(session: Session): void {
        this.sessions.delete(session.socket);

        if (!session.room || !session.peer) {
            return;
        }

        // Não destrói na hora: a mídia continua viva e a pessoa tem uma janela para
        // reconectar a sinalização sem cair da chamada.
        session.room.orphanPeer(session.peer);
    }
}

/**
 * O IP de verdade vem do nginx. Confiar no cabeçalho só é seguro porque o SFU escuta em
 * 127.0.0.1: quem chega aqui já passou pelo proxy, e ninguém fala com ele direto.
 */
const addressOf = (request: IncomingMessage): string => {
    const real = request.headers['x-real-ip']?.toString().trim();
    const forwarded = request.headers['x-forwarded-for'];
    const first = (Array.isArray(forwarded) ? forwarded[0] : forwarded)?.split(',')[0]?.trim();

    return real || first || request.socket.remoteAddress || 'desconhecido';
};
