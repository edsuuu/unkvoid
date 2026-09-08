export class PresenceClient extends EventTarget {
    /**
     * Pede o convite da presença ao servidor: `{ url, token }`.
     *
     * A web fala por sessão e CSRF; o app fala por Bearer contra outro domínio. É a
     * única diferença entre os dois, então é a única coisa que se injeta — o resto do
     * socket, da reconexão e do desenho é igual nos dois.
     */
    static async webTicket(serverId) {
        const response = await fetch(`/api/servidores/${serverId}/presenca`, {
            method: 'POST',
            headers: {
                'X-CSRF-TOKEN': document.querySelector('meta[name=csrf-token]')?.content ?? '',
                Accept: 'application/json',
            },
        });

        if (!response.ok) {
            throw new Error(`presence rejected (${response.status})`);
        }

        return response.json();
    }

    constructor(ticket = PresenceClient.webTicket) {
        super();
        this.ticket = ticket;
        this.socket = null;
        this.serverId = null;
        this.closedByUs = false;
        this.attempt = 0;
        this.timer = null;
        this.opening = null;
    }

    emit(name, detail) {
        this.dispatchEvent(new CustomEvent(name, { detail }));
    }

    /**
     * Idempotent: the caller fires on every DOM change, so repeating for the
     * same server (even while the connection is opening) must not restart anything.
     */
    async watch(serverId) {
        const jaConectado = this.serverId === serverId
            && (this.socket?.readyState === WebSocket.OPEN || this.opening);

        if (jaConectado) {
            return this.opening ?? undefined;
        }

        this.stop();
        this.serverId = serverId;
        this.closedByUs = false;
        this.attempt = 0;
        this.opening = this.open().finally(() => {
            this.opening = null;
        });

        return this.opening;
    }

    async open() {
        const { url, token } = await this.ticket(this.serverId);

        await new Promise((resolve, reject) => {
            this.socket = new WebSocket(url);
            this.socket.onerror = () => reject(new Error('presence socket failed'));
            this.socket.onclose = () => this.reopen();
            this.socket.onmessage = message => {
                const payload = JSON.parse(message.data);

                if (payload.event === 'presence') {
                    this.emit('presence', payload.data.channels);
                }

                if (payload.ok && payload.data?.channels) {
                    this.emit('presence', payload.data.channels);
                }
            };
            this.socket.onopen = () => {
                this.attempt = 0;
                this.socket.send(JSON.stringify({ id: 1, action: 'watchServer', data: { token } }));
                resolve();
            };
        });
    }

    reopen() {
        if (this.closedByUs || this.attempt >= 8) {
            return;
        }

        const delay = Math.min(1000 * 2 ** this.attempt, 10000);

        this.attempt += 1;
        this.timer = setTimeout(() => void this.open().catch(() => this.reopen()), delay);
    }

    stop() {
        this.closedByUs = true;
        clearTimeout(this.timer);

        if (this.socket) {
            this.socket.onclose = null;
            this.socket.close();
        }

        this.socket = null;
        this.serverId = null;
        this.opening = null;
    }
}
