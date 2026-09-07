/**
 * Laravel API client. The app is useless without a server, so every network failure
 * sends it to the reconnect screen instead of letting the interface show stale data.
 */
export class Api {
    // VITE_API_BASE points a local build at a local stack; the localStorage override
    // stays for poking at a shipped build without rebuilding it.
    static BASE = import.meta.env.VITE_API_BASE ?? localStorage.getItem('api:base') ?? 'https://discord.unkvoid.com';

    constructor(onOffline) {
        this.token = localStorage.getItem('api:token');
        this.onOffline = onOffline;
    }

    get authenticated() {
        return Boolean(this.token);
    }

    async request(path, { method = 'GET', body } = {}) {
        let resposta;

        try {
            resposta = await fetch(`${Api.BASE}/api/${path}`, {
                method,
                headers: {
                    Accept: 'application/json',
                    'Content-Type': 'application/json',
                    ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
                },
                body: body ? JSON.stringify(body) : undefined,
            });
        } catch {
            this.onOffline?.();

            throw new Error('no connection to the server');
        }

        if (resposta.status === 401) {
            this.forget();

            throw new Error('session expired');
        }

        const dados = await resposta.json().catch(() => ({}));

        if (! resposta.ok) {
            throw new Error(dados.message ?? `the server rejected the request (${resposta.status})`);
        }

        return dados;
    }

    async login(email, password) {
        const dados = await this.request('login', {
            method: 'POST',
            body: { email, password, device: `desktop-${navigator.platform}` },
        });

        this.token = dados.token;
        localStorage.setItem('api:token', dados.token);

        return dados.user.data ?? dados.user;
    }

    forget() {
        this.token = null;
        localStorage.removeItem('api:token');
    }

    me() {
        return this.request('me').then(dados => dados.data);
    }

    servers() {
        return this.request('servers').then(dados => dados.data);
    }

    server(id) {
        return this.request(`servers/${id}`).then(dados => dados.data);
    }

    createServer(name) {
        return this.request('servers', { method: 'POST', body: { name } }).then(dados => dados.data);
    }

    messages(channelId) {
        return this.request(`channels/${channelId}/messages`).then(dados => dados.data);
    }

    sendMessage(channelId, content) {
        return this.request(`channels/${channelId}/messages`, { method: 'POST', body: { content } })
            .then(dados => dados.data);
    }

    voiceToken(channelId) {
        return this.request(`voice/${channelId}/token`, { method: 'POST' });
    }

    presenceToken(serverId) {
        return this.request(`servers/${serverId}/presence`, { method: 'POST' });
    }
}
