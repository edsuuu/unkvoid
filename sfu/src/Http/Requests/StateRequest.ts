import { Request } from './Request.js';

/**
 * Microfone e áudio mudos, contados pelo próprio dono.
 *
 * Mutar não fecha o producer — o áudio é cortado antes de sair — então o servidor não
 * tem como perceber. Sem este aviso, ninguém vê o ícone de mudo de ninguém.
 */
export class StateRequest extends Request {
    protected override validate(): void {
        this.boolean('muted');
        this.boolean('deafened');
    }

    muted(): boolean {
        return this.boolean('muted');
    }

    deafened(): boolean {
        return this.boolean('deafened');
    }
}
