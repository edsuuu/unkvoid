import { ValidationException } from '../../Exceptions/ApiException.js';
import { Request } from './Request.js';

/** Códigos podem ser nomes legíveis; o limite evita chaves enormes no mapa e no protocolo. */
const CODE = /^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$/;

const NAME_MAX = 40;

export class JoinRequest extends Request {
    protected override validate(): void {
        if (! CODE.test(this.string('room'))) {
            throw new ValidationException('room code must be 3-32 characters of a-z0-9, with optional hyphens');
        }

        if (this.string('name').trim().length > NAME_MAX) {
            throw new ValidationException(`name must be at most ${NAME_MAX} characters`);
        }
    }

    roomCode(): string {
        return this.string('room');
    }

    name(): string {
        return this.string('name').trim();
    }

    /**
     * A chave que prova ser a mesma pessoa de antes da queda. Sai apenas na resposta do
     * join, nunca no broadcast — o `peerId` a sala inteira conhece, e se ele bastasse
     * qualquer um derrubaria qualquer um entrando com o id alheio.
     */
    resumeKey(): string | null {
        return typeof this.data.resumeKey === 'string' ? this.data.resumeKey : null;
    }

    /**
     * Só o cliente sabe se os transportes dele continuam vivos. Depois de reabrir ele é
     * novo em folha, então pede sessão limpa mesmo que o servidor ainda guarde a antiga
     * — retomá-la deixaria o cliente sem transporte nenhum.
     */
    wantsResume(): boolean {
        return this.data.resume === true;
    }
}
