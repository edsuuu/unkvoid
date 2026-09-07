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
        let answer;

        try {
            answer = await fetch(`${Api.BASE}/api/${path}`, {
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

        if (answer.status === 401) {
            this.forget();

            throw new Error('session expired');
        }

        const data = await answer.json().catch(() => ({}));

        if (! answer.ok) {
            throw new Error(data.message ?? `the server rejected the request (${answer.status})`);
        }

        return data;
    }

    async login(email, password) {
        const data = await this.request('login', {
            method: 'POST',
            body: { email, password, device: `desktop-${navigator.platform}` },
        });

        this.token = data.token;
        localStorage.setItem('api:token', data.token);

        return data.user.data ?? data.user;
    }

    forget() {
        this.token = null;
        localStorage.removeItem('api:token');
    }

    me() {
        return this.request('me').then(data => data.data);
    }

    servers() {
        return this.request('servers').then(data => data.data);
    }

    server(id) {
        return this.request(`servers/${id}`).then(data => data.data);
    }

    createServer(name) {
        return this.request('servers', { method: 'POST', body: { name } }).then(data => data.data);
    }

    messages(channelId) {
        return this.request(`channels/${channelId}/messages`).then(data => data.data);
    }

    sendMessage(channelId, content) {
        return this.request(`channels/${channelId}/messages`, { method: 'POST', body: { content } })
            .then(data => data.data);
    }

    voiceToken(channelId) {
        return this.request(`voice/${channelId}/token`, { method: 'POST' });
    }

    presenceToken(serverId) {
        return this.request(`servers/${serverId}/presence`, { method: 'POST' });
    }
}
