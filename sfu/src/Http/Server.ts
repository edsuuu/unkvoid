import { createServer, type IncomingMessage } from 'node:http';

import { WebSocketServer, type RawData, type WebSocket } from 'ws';

import { config } from '../config.js';
import { RoomRegistry } from '../Services/RoomRegistry.js';
import type { Session } from '../types.js';
import { Kernel } from './Kernel.js';

type Payload = { id?: number; action?: string; data?: Record<string, unknown> };

const WINDOW_MS = 60_000;

export class Server {
    private readonly registry = new RoomRegistry();

    private readonly kernel = new Kernel(this.registry);

    private readonly sessions = new Map<WebSocket, Session>();

    private readonly recent = new Map<string, number[]>();

    async start(): Promise<void> {
        await this.registry.boot();

        const http = createServer((request, response) => {
            if (request.url !== '/health') {
                response.writeHead(404).end();

                return;
            }

            response.writeHead(200, { 'content-type': 'application/json' });
            response.end(JSON.stringify({ ok: true, appVersion: config.appVersion, ...this.registry.stats() }));
        });

        new WebSocketServer({ server: http, path: config.path })
            .on('connection', (socket, request) => this.accept(socket, request));

        http.listen(config.listenPort, config.listenHost, () =>
            console.log(`[INFO] SFU em ${config.listenHost}:${config.listenPort}${config.path} · media on port ${config.mediaPort}`));
    }

    private accept(socket: WebSocket, request: IncomingMessage): void {
        if (this.tooMany(addressOf(request))) {
            socket.close(1013, 'too many connections — try again in a minute');

            return;
        }

        const session: Session = { socket, room: null, peer: null };

        this.sessions.set(socket, session);

        socket.on('message', raw => void this.handle(session, raw));
        socket.on('close', () => this.release(session));
        socket.on('error', error => console.error('[ERROR] socket', error.message));
    }

    private tooMany(address: string): boolean {
        const now = Date.now();
        const hits = (this.recent.get(address) ?? []).filter(at => now - at < WINDOW_MS);

        hits.push(now);
        this.recent.set(address, hits);

        // ponytail: varre o mapa inteiro quando ele cresce. Um LRU só valeria a pena na
        // ordem de milhares de IPs por minuto, que não é o tamanho disto.
        if (this.recent.size > 1000) {
            for (const [known, times] of this.recent) {
                if (times.every(at => now - at >= WINDOW_MS)) {
                    this.recent.delete(known);
                }
            }
        }

        return hits.length > config.connectionsPerMinute;
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
            session.socket.send(JSON.stringify({ id: payload.id, ok: false, status, error: message }));
        }
    }

    private release(session: Session): void {
        this.sessions.delete(session.socket);

        if (! session.room || ! session.peer) {
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
    const forwarded = request.headers['x-forwarded-for'];
    const first = (Array.isArray(forwarded) ? forwarded[0] : forwarded)?.split(',')[0]?.trim();

    return first || request.socket.remoteAddress || 'desconhecido';
};
