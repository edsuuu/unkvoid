import { createServer } from 'node:http';

import { WebSocketServer } from 'ws';

import { config } from '../config.js';
import { RoomRegistry } from '../Services/RoomRegistry.js';
import { TokenVerifier } from '../Services/TokenVerifier.js';
import { Kernel } from './Kernel.js';

export class Server {
    constructor() {
        this.registry = new RoomRegistry();
        this.kernel = new Kernel(this.registry, new TokenVerifier(config.tokenSecret));
        this.sessions = new Map();
    }

    async start() {
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

    accept(socket) {
        const session = { socket, room: null, peer: null };

        this.sessions.set(socket, session);

        socket.on('message', raw => this.handle(session, raw));
        socket.on('close', () => this.release(session));
        socket.on('error', error => console.error('[ERRO] socket', error.message));
    }

    async handle(session, raw) {
        let payload;

        try {
            payload = JSON.parse(raw);
        } catch {
            console.error('[WARN] mensagem inválida descartada');

            return;
        }

        try {
            const data = await this.kernel.dispatch(payload.action, payload.data, session);

            session.socket.send(JSON.stringify({ id: payload.id, ok: true, data }));
        } catch (exception) {
            const status = Kernel.statusOf(exception);

            console.error(`[ERRO] ${payload.action} (${status}): ${exception.message}`);
            session.socket.send(JSON.stringify({ id: payload.id, ok: false, status, error: exception.message }));
        }
    }

    release(session) {
        this.sessions.delete(session.socket);

        if (! session.room) {
            return;
        }

        session.room.removePeer(session.peer);
        this.registry.release(session.room);
    }
}
