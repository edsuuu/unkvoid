import { randomUUID } from 'node:crypto';

import { Request } from './Request.js';
import { ValidationException } from '../../Exceptions/ApiException.js';
import { Signature, type JoinClaims } from '../../Services/Signature.js';

const LEGACY_CODE = /^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$/;

/** O app manda um UUID. Qualquer outra coisa é sorteada aqui: ela vai para a auditoria. */
const INSTALL_ID = /^[A-Za-z0-9-]{1,64}$/;

/**
 * Entrar na sala é apresentar o token que o Laravel assinou. Quem pode entrar, com que
 * nome e o que pode produzir foi decidido lá, contra o banco; aqui só se confere a assinatura e a
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
        // instalado que não sabe pedir um. Entra como visitante, com tudo liberado.
        // Apagar quando o app com login estiver publicado nos três sistemas.
        const room = this.string('room');
        const name = this.string('name').trim();

        // 26 caracteres é um ULID: canal de servidor, que só entra com token assinado.
        if (room.length === 26 || !LEGACY_CODE.test(room) || name === '' || name.length > 40) {
            throw new ValidationException('field token is required');
        }

        const installId =
            typeof this.data.installId === 'string' && INSTALL_ID.test(this.data.installId)
                ? this.data.installId
                : randomUUID();

        this.claims = {
            room,
            sub: `guest:${installId}`,
            name,
            exp: 0,
            can: ['speak', 'stream', 'video'],
        };
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

    public can(): string[] {
        return this.claims.can;
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
