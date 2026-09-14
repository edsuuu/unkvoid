/** O `fetch` com o token do Sanctum e os erros do Laravel já lidos. */
export class ApiClient {
    static TOKEN_KEY = 'unkvoid:token';

    constructor(server) {
        this.server = server;
        this.token = localStorage.getItem(ApiClient.TOKEN_KEY);
    }

    setToken(token) {
        this.token = token;

        if (token) {
            localStorage.setItem(ApiClient.TOKEN_KEY, token);
        } else {
            localStorage.removeItem(ApiClient.TOKEN_KEY);
        }
    }

    /** Falhou: o erro carrega `status` e `errors`, que é o que decide o aviso. */
    async request(method, path, body) {
        const headers = { Accept: 'application/json' };

        if (this.token) {
            headers.Authorization = `Bearer ${this.token}`;
        }

        if (body !== undefined) {
            headers['Content-Type'] = 'application/json';
        }

        const response = await fetch(`${this.server}${path}`, {
            method,
            headers,
            body: body === undefined ? undefined : JSON.stringify(body),
        });

        // Um 502 do nginx vem em HTML: o status é a informação, o corpo não.
        const json = /json/i.test(response.headers.get('content-type') ?? '');
        const data = json ? await response.json() : null;

        if (! response.ok) {
            // O 422 traz o primeiro erro de campo; o resto traz só `message`.
            const firstField = Object.values(data?.errors ?? {})[0]?.[0];
            const message = firstField ?? data?.message ?? `o servidor respondeu ${response.status}`;

            throw Object.assign(new Error(message), { status: response.status, errors: data?.errors ?? {} });
        }

        // O contrato é sem envelope, mas um `Resource` do Laravel embrulha em `data` até
        // alguém chamar `withoutWrapping`. Aceitar os dois custa esta linha; nenhuma
        // resposta do contrato tem um campo próprio chamado `data`.
        return data && typeof data === 'object' && Object.keys(data).length === 1 && 'data' in data ? data.data : data;
    }

    get(path) {
        return this.request('GET', path);
    }

    post(path, body = {}) {
        return this.request('POST', path, body);
    }

    patch(path, body) {
        return this.request('PATCH', path, body);
    }

    put(path, body) {
        return this.request('PUT', path, body);
    }

    delete(path) {
        return this.request('DELETE', path);
    }
}
