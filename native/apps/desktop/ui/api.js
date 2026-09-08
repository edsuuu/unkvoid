/**
 * O servidor, resumido ao que o app ainda precisa dele: emitir o token da sala.
 *
 * O token do SFU é assinado com um segredo que não pode viajar dentro do binário — daí
 * o único endpoint. Não há conta, sessão nem cadastro: o app manda um nome e um código
 * de sala, e recebe de volta a permissão de entrar naquela sala.
 */
export class Api {
    // VITE_API_BASE aponta um build local para uma pilha local; o override no
    // localStorage serve para cutucar um build já pronto sem recompilar.
    static BASE = import.meta.env.VITE_API_BASE ?? localStorage.getItem('api:base') ?? 'https://discord.unkvoid.com';

    constructor(onOffline) {
        this.onOffline = onOffline;
    }

    /** Sem `code`, o servidor sorteia uma sala nova e devolve o código dela. */
    async room(name, code = null) {
        let answer;

        try {
            answer = await fetch(`${Api.BASE}/api/rooms`, {
                method: 'POST',
                headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
                body: JSON.stringify({ name, room: code }),
            });
        } catch {
            this.onOffline?.();

            throw new Error('sem conexão com o servidor');
        }

        const data = await answer.json().catch(() => ({}));

        if (! answer.ok) {
            // 422 do Laravel traz o motivo por campo; a mensagem solta é genérica.
            throw new Error(
                Object.values(data.errors ?? {}).flat()[0]
                ?? data.message
                ?? `o servidor recusou (${answer.status})`,
            );
        }

        return data;
    }
}
