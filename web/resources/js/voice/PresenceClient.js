export class PresenceClient extends EventTarget {
    constructor() {
        super();
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
     * Idempotente: o chamador dispara a cada mudança de DOM, então repetir para o
     * mesmo servidor (mesmo com a conexão ainda abrindo) não pode reiniciar nada.
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
        const response = await fetch(`/api/servidores/${this.serverId}/presenca`, {
            method: 'POST',
            headers: {
                'X-CSRF-TOKEN': document.querySelector('meta[name=csrf-token]')?.content ?? '',
                Accept: 'application/json',
            },
        });

        if (!response.ok) {
            throw new Error(`presença recusada (${response.status})`);
        }

        const { url, token } = await response.json();

        await new Promise((resolve, reject) => {
            this.socket = new WebSocket(url);
            this.socket.onerror = () => reject(new Error('socket de presença falhou'));
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
