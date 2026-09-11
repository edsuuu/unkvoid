import { Request } from './Request.js';
import { Signature, type JoinClaims } from '../../Services/Signature.js';

/**
 * Entrar na sala é apresentar o token que o Laravel assinou. Quem pode entrar, com que
 * nome e se é dono foi decidido lá, contra o banco; aqui só se confere a assinatura e a
 * validade. Nenhuma chamada de rede no caminho do join.
 */
export class JoinRequest extends Request {
    declare private claims: JoinClaims;

    protected override validate(): void {
        this.claims = Signature.claims(this.string('token'));
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
