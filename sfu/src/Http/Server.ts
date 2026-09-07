import { createServer } from 'node:http';

import { WebSocketServer, type RawData, type WebSocket } from 'ws';

import { config } from '../config.js';
import { RoomRegistry } from '../Services/RoomRegistry.js';
import { TokenVerifier } from '../Services/TokenVerifier.js';
import type { Session } from '../types.js';
import { Kernel } from './Kernel.js';

type Payload = { id?: number; action?: string; data?: Record<string, unknown> };

export class Server {
    private readonly registry = new RoomRegistry();

    private readonly kernel: Kernel;

    private readonly sessions = new Map<WebSocket, Session>();

    constructor() {
        this.kernel = new Kernel(this.registry, new TokenVerifier(config.tokenSecret));
    }

    async start(): Promise<void> {
        await this.registry.boot();

        const http = createServer((request, response) => {
            if (request.url !== '/health') {
                response.writeHead(404).end();

                return;
            }

            response.writeHead(200, { 'content-type': 'application/json' });
            response.end(JSON.stringify({ ok: true, ...this.registry.stats() }));
        });

        new WebSocketServer({ server: http, path: config.path })
            .on('connection', socket => this.accept(socket));

        http.listen(config.listenPort, config.listenHost, () =>
            console.log(`[INFO] SFU em ${config.listenHost}:${config.listenPort}${config.path} · mídia na porta ${config.mediaPort}`));
    }

    private accept(socket: WebSocket): void {
        const session: Session = { socket, room: null, peer: null };

        this.sessions.set(socket, session);

        socket.on('message', raw => void this.handle(session, raw));
        socket.on('close', () => this.release(session));
        socket.on('error', error => console.error('[ERRO] socket', error.message));
    }

    private async handle(session: Session, raw: RawData): Promise<void> {
        let payload: Payload;

        try {
            payload = JSON.parse(raw.toString()) as Payload;
        } catch {
            console.error('[WARN] mensagem inválida descartada');

            return;
        }

        try {
            const data = await this.kernel.dispatch(payload.action ?? '', payload.data, session);

            session.socket.send(JSON.stringify({ id: payload.id, ok: true, data }));
        } catch (exception) {
            const status = Kernel.statusOf(exception);
            const message = exception instanceof Error ? exception.message : 'erro inesperado';

            console.error(`[ERRO] ${payload.action} (${status}): ${message}`);
            session.socket.send(JSON.stringify({ id: payload.id, ok: false, status, error: message }));
        }
    }

    private release(session: Session): void {
        this.sessions.delete(session.socket);

        if (! session.room || ! session.peer) {
            return;
        }

        session.room.removePeer(session.peer);
        this.registry.release(session.room);
    }
}
