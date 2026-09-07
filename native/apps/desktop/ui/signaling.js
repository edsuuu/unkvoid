/**
 * Sinalização pelo WebSocket do SFU.
 *
 * Reusa a sala e a autenticação que já existem: o SFU não entende o conteúdo do
 * sinal, só entrega de um participante a outro. O remetente vem da sessão dele,
 * então ninguém consegue se passar por outra pessoa.
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
            this.socket.onerror = () => rejeitar(new Error('não abriu o WebSocket do SFU'));
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

    /** Entrega um sinal a um participante específico da sala. */
    signal(to, kind, payload) {
        return this.request('signal', { to, kind, payload });
    }

    close() {
        this.socket?.close();
    }
}
