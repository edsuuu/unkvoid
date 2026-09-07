/**
 * Signaling through the SFU WebSocket.
 *
 * Reuses the existing room and authentication: the SFU does not understand the
 * signal contents, it only delivers them from one participant to another. The
 * sender comes from their session, so nobody can impersonate someone else.
 */
export class Signaling extends EventTarget {
    constructor() {
        super();
        this.socket = null;
        this.pendentes = new Map();
        this.proximoId = 1;
        this.peerId = null;
    }

    emitir(nome, detalhe) {
        this.dispatchEvent(new CustomEvent(nome, { detail: detalhe }));
    }

    async connect(url, token) {
        await new Promise((resolver, rejeitar) => {
            this.socket = new WebSocket(url);
            this.socket.onerror = () => rejeitar(new Error('could not open the SFU WebSocket'));
            this.socket.onopen = resolver;
            this.socket.onclose = () => this.emitir('closed');
            this.socket.onmessage = mensagem => this.receber(JSON.parse(mensagem.data));
        });

        const entrada = await this.request('join', { token });

        this.peerId = entrada.peerId;

        return entrada;
    }

    receber(mensagem) {
        if (mensagem.event) {
            this.emitir(mensagem.event, mensagem.data);

            return;
        }

        const aguardando = this.pendentes.get(mensagem.id);

        if (! aguardando) {
            return;
        }

        this.pendentes.delete(mensagem.id);
        mensagem.ok ? aguardando.resolver(mensagem.data) : aguardando.rejeitar(new Error(mensagem.error));
    }

    request(action, data = {}) {
        const id = this.proximoId++;

        return new Promise((resolver, rejeitar) => {
            this.pendentes.set(id, { resolver, rejeitar });
            this.socket.send(JSON.stringify({ id, action, data }));
        });
    }

    /** Delivers a signal to a specific participant in the room. */
    signal(to, kind, payload) {
        return this.request('signal', { to, kind, payload });
    }

    close() {
        this.socket?.close();
    }
}
