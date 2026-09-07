import { Request } from './Request.js';

export class JoinRequest extends Request {
    protected override validate(): void {
        this.string('token');
    }

    token(): string {
        return this.string('token');
    }

    /**
     * Só o cliente sabe se ainda tem os transports vivos. Depois de um F5 ele é
     * novo em folha, então pede sessão limpa mesmo que o servidor ainda guarde a
     * antiga — retomar ali deixaria o cliente sem transport nenhum.
     */
    wantsResume(): boolean {
        return this.data.resume === true;
    }
}
