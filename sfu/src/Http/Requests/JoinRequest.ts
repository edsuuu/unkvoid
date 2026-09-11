import { randomUUID } from 'node:crypto';

import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';
import { Signature, type JoinClaims } from '../../Services/Signature.js';

const LEGACY_CODE = /^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$/;

/**
 * Entrar na sala é apresentar o token que o Laravel assinou. Quem pode entrar, com que
 * nome e se é dono foi decidido lá, contra o banco; aqui só se confere a assinatura e a
 * validade. Nenhuma chamada de rede no caminho do join.
 */
export class JoinRequest extends Request {
    declare private claims: JoinClaims;

    protected override validate(): void {
        if (typeof this.data.token === 'string') {
            this.claims = Signature.claims(this.data.token);

            return;
        }

        // ponytail: o join antigo, sem token, continua aceito enquanto houver app
        // instalado que não sabe pedir um. Entra como visitante, nunca como dono.
        // Apagar quando o app com login estiver publicado nos três sistemas.
        const room = this.string('room');
        const name = this.string('name').trim();

        if (!LEGACY_CODE.test(room) || name === '' || name.length > 40) {
            throw new ValidationException('field token is required');
        }

        const installId =
            typeof this.data.installId === 'string' ? this.data.installId : randomUUID();

        this.claims = { room, sub: `guest:${installId}`, name, owner: false, exp: 0 };
    }

    public roomCode(): string {
        return this.claims.room;
    }

    public name(): string {
        return this.claims.name.trim();
    }

    public userId(): string {
        return this.claims.sub;
    }

    public isOwner(): boolean {
        return this.claims.owner;
    }

    /**
     * A chave que prova ser a mesma pessoa de antes da queda. Sai apenas na resposta do
     * join, nunca no broadcast — o `peerId` a sala inteira conhece, e se ele bastasse
     * qualquer um derrubaria qualquer um entrando com o id alheio.
     */
    public resumeKey(): string | null {
        return typeof this.data.resumeKey === 'string' ? this.data.resumeKey : null;
    }

    /**
     * Só o cliente sabe se os transportes dele continuam vivos. Depois de reabrir ele é
     * novo em folha, então pede sessão limpa mesmo que o servidor ainda guarde a antiga
     * — retomá-la deixaria o cliente sem transporte nenhum.
     */
    public wantsResume(): boolean {
        return this.data.resume === true;
    }
}
