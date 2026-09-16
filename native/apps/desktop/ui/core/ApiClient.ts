type LaravelBody = { message?: string; errors?: Record<string, string[]>; data?: unknown } | null;

export class ApiClient {
    static readonly TOKEN_KEY = 'unkvoid:token';

    readonly server: string;
    token: string | null;

    constructor(server: string) {
        this.server = server;
        this.token = localStorage.getItem(ApiClient.TOKEN_KEY);
    }

    setToken(token: string | null): void {
        this.token = token;

        if (token) {
            localStorage.setItem(ApiClient.TOKEN_KEY, token);
        } else {
            localStorage.removeItem(ApiClient.TOKEN_KEY);
        }
    }

    async request<Result = unknown>(method: string, path: string, body?: unknown): Promise<Result> {
        const headers: Record<string, string> = { Accept: 'application/json' };
        const form = body instanceof FormData;

        if (this.token) {
            headers.Authorization = `Bearer ${this.token}`;
        }

        if (body !== undefined && ! form) {
            headers['Content-Type'] = 'application/json';
        }

        const response = await fetch(`${this.server}${path}`, {
            method,
            headers,
            body: body === undefined ? undefined : form ? body : JSON.stringify(body),
        });

        const isJson = /json/i.test(response.headers.get('content-type') ?? '');
        const data = (isJson ? await response.json() : null) as LaravelBody;

        if (! response.ok) {
            const firstFieldError = Object.values(data?.errors ?? {})[0]?.[0];
            const message = firstFieldError ?? data?.message ?? `o servidor respondeu ${response.status}`;

            throw Object.assign(new Error(message), { status: response.status, errors: data?.errors ?? {} });
        }

        const wrappedByResource = data !== null && typeof data === 'object' && Object.keys(data).length === 1 && 'data' in data;

        return (wrappedByResource ? data.data : data) as Result;
    }

    upload<Result = unknown>(path: string, field: string, file: File): Promise<Result> {
        const form = new FormData();

        form.append(field, file);

        return this.request<Result>('POST', path, form);
    }

    get<Result = unknown>(path: string): Promise<Result> {
        return this.request<Result>('GET', path);
    }

    post<Result = unknown>(path: string, body: unknown = {}): Promise<Result> {
        return this.request<Result>('POST', path, body);
    }

    patch<Result = unknown>(path: string, body: unknown): Promise<Result> {
        return this.request<Result>('PATCH', path, body);
    }

    put<Result = unknown>(path: string, body: unknown): Promise<Result> {
        return this.request<Result>('PUT', path, body);
    }

    delete<Result = unknown>(path: string): Promise<Result> {
        return this.request<Result>('DELETE', path);
    }
}
