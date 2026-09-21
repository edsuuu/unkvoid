import express, { type NextFunction, type Request, type Response } from 'express';
import { createServer, type IncomingMessage, type Server as HttpServer } from 'node:http';
import { WebSocketServer, type RawData, type WebSocket } from 'ws';

import { config } from './Config/index.js';
import { NotFoundException } from './Exceptions/ApiException.js';
import { cors } from './Http/Middleware/Cors.js';
import type { SignedRequest } from './Http/Middleware/VerifySignature.js';
import type { Session } from './Http/Request/Request.js';
import { HttpRouter } from './Routers/HttpRouter.js';
import { Broadcaster } from './Services/Broadcaster.js';
import { Kernel } from './Services/Kernel.js';
import { RoomRegistry } from './Services/RoomRegistry.js';
import { Subscriptions } from './Services/Subscriptions.js';

type Payload = { id?: number; action?: string; data?: Record<string, unknown> };

const WINDOW_MS = 60_000;

const MAX_BODY = '16kb';

export class App {
    private readonly registry = new RoomRegistry();

    private readonly subscriptions = new Subscriptions();

    private readonly broadcaster = new Broadcaster(this.subscriptions);

    private readonly kernel = new Kernel(this.registry, this.subscriptions, this.broadcaster);

    private readonly sessions = new Map<WebSocket, Session>();

    private readonly recent = new Map<string, number[]>();

    private readonly alive = new WeakSet<WebSocket>();

    public async boot(): Promise<HttpServer> {
        await this.registry.boot();

        const http = createServer(this.express());

        this.listenWebSockets(http);

        return http;
    }

    private express(): express.Express {
        const app = express();

        app.disable('x-powered-by');

        app.use(cors);

        app.use(
            express.json({
                limit: MAX_BODY,

                verify: (request: SignedRequest, _response, buffer: Buffer) => {
                    request.rawBody = buffer.toString();
                },
            }),
        );

        app.use(HttpRouter.routes(this.registry, this.broadcaster));

        app.use((_request, _response, next: NextFunction) => next(new NotFoundException()));

        app.use((exception: Error, request: Request, response: Response, _next: NextFunction) => {
            const status = exception instanceof SyntaxError ? 422 : Kernel.statusOf(exception);

            if (status === 500) {
                console.error(
                    `[ERROR] http ${request.method} ${request.originalUrl}: ${exception.message}`,
                );
            }

            response.status(status).json({ ok: false, error: exception.message });
        });

        return app;
    }

    private listenWebSockets(http: HttpServer): void {
        const websockets = new WebSocketServer({ server: http, path: config.path });

        websockets.on('connection', (socket, request) => this.accept(socket, request));

        setInterval(() => {
            for (const socket of websockets.clients) {
                if (!this.alive.has(socket)) {
                    socket.terminate();

                    continue;
                }

                this.alive.delete(socket);
                socket.ping();
            }
        }, config.heartbeatMs).unref();
    }

    private accept(socket: WebSocket, request: IncomingMessage): void {
        const ip = addressOf(request);

        if (this.tooMany(ip)) {
            socket.close(1013, 'too many connections — try again in a minute');

            return;
        }

        const session: Session = { socket, ip, room: null, peer: null, identity: null };

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

        if (!over) {
            hits.push(now);
        }

        this.recent.set(address, hits);

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

    /**
     * Quem cai deixa de ouvir tudo, e some da presença de cada canal — mas só quando era
     * a última conexão daquela pessoa: quem está com o app aberto em duas máquinas
     * continua na lista pela outra.
     */
    private dropSubscriptions(session: Session): void {
        const channels = this.subscriptions.removeSocket(session.socket);
        const identity = session.identity;

        if (!identity) {
            return;
        }

        for (const channel of channels) {
            if (this.subscriptions.countFor(channel, identity.userId) === 0) {
                this.broadcaster.send(channel, 'presence.leaving', { id: identity.userId });
            }
        }
    }

    private release(session: Session): void {
        this.sessions.delete(session.socket);
        this.dropSubscriptions(session);

        if (!session.room || !session.peer) {
            return;
        }

        if (session.peer.socket !== session.socket) {
            return;
        }

        session.room.orphanPeer(session.peer);
    }
}

const addressOf = (request: IncomingMessage): string => {
    const real = request.headers['x-real-ip']?.toString().trim();
    const forwarded = request.headers['x-forwarded-for'];
    const first = (Array.isArray(forwarded) ? forwarded[0] : forwarded)?.split(',')[0]?.trim();

    return real || first || request.socket.remoteAddress || 'desconhecido';
};
