import WebSocket from 'ws';

/**
 * O WebSocket do SFU no envelope do app (`protocol.rs`): `{ id, action, data }` sobe, a
 * resposta volta com o mesmo `id`, e o que vem sem `id` é evento da sala.
 */
export class SignalClient {
    constructor(url) {
        this.url = url;
        this.nextId = 1;
        this.pending = new Map();
        this.listeners = new Set();
        this.closeListeners = new Set();
        this.closeCode = null;
        this.closed = false;
    }

    open(timeoutMs = 5000) {
        this.socket = new WebSocket(this.url, { handshakeTimeout: timeoutMs });

        this.socket.on('message', raw => this.receive(raw));
        this.socket.on('close', (code, reason) => {
            this.closed = true;
            this.closeCode = code;

            for (const [, waiter] of this.pending) {
                waiter.reject(Object.assign(new Error(`socket closed (${code})`), { status: 0 }));
            }

            this.pending.clear();

            for (const listener of this.closeListeners) {
                listener(code, reason.toString());
            }
        });

        return new Promise((resolve, reject) => {
            this.socket.once('open', () => resolve(this));
            this.socket.once('error', error => {
                this.closed = true;
                reject(error);
            });
        });
    }

    receive(raw) {
        let payload;

        try {
            payload = JSON.parse(raw.toString());
        } catch {
            return;
        }

        if (payload.id !== undefined && payload.id !== null) {
            const waiter = this.pending.get(payload.id);

            this.pending.delete(payload.id);

            if (!waiter) {
                return;
            }

            if (payload.ok) {
                waiter.resolve(payload.data ?? {});
            } else {
                waiter.reject(Object.assign(new Error(`${waiter.action} (${payload.status}): ${payload.error}`), { status: payload.status }));
            }

            return;
        }

        if (payload.event) {
            for (const listener of this.listeners) {
                listener(payload.event, payload.data ?? {});
            }
        }
    }

    call(action, data = {}, timeoutMs = 10_000) {
        if (this.closed || this.socket.readyState !== WebSocket.OPEN) {
            return Promise.reject(Object.assign(new Error(`${action}: socket is not open`), { status: 0 }));
        }

        const id = this.nextId++;

        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.pending.delete(id);
                reject(Object.assign(new Error(`${action}: no answer in ${timeoutMs} ms`), { status: -1 }));
            }, timeoutMs);

            this.pending.set(id, {
                action,
                resolve: value => {
                    clearTimeout(timer);
                    resolve(value);
                },
                reject: error => {
                    clearTimeout(timer);
                    reject(error);
                },
            });

            this.socket.send(JSON.stringify({ id, action, data }));
        });
    }

    onEvent(listener) {
        this.listeners.add(listener);
    }

    onClose(listener) {
        this.closeListeners.add(listener);
    }

    /** Larga o socket sem fechamento educado, como o app que desiste de um socket mudo. */
    drop() {
        this.listeners.clear();
        this.closeListeners.clear();
        this.socket?.terminate();
    }

    close() {
        this.listeners.clear();
        this.closeListeners.clear();
        this.socket?.close();
    }
}
