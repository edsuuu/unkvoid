import { Signature } from './Signature.js';
import { config } from '../Config/index.js';
import { ForbiddenException, ServiceUnavailableException } from '../Exceptions/ApiException.js';

const PATH = '/api/sfu/authorize';

type Answer = { allowed?: unknown; name?: unknown };

/**
 * Quem decide se alguém pode ouvir um canal é o Laravel, que tem o banco, os cargos e as
 * sobrescritas. O SFU pergunta e obedece — é o mesmo papel do `broadcasting/auth`.
 *
 * ponytail: uma ida ao Laravel por inscrição, sem cache. Inscrição acontece ao abrir um
 * servidor, não por mensagem, então o volume é baixo. Se virar gargalo, o cache tem de
 * ser invalidado quando um cargo muda — por isso não começa com um.
 */
export class Authorizer {
    public static async allows(userId: string, channel: string): Promise<string | null> {
        if (config.laravelUrl === '') {
            throw new ServiceUnavailableException('SFU_LARAVEL_URL is not configured');
        }

        const at = Math.floor(Date.now() / 1000);
        const body = JSON.stringify({ sub: userId, channel, at });

        let response: Response;

        try {
            response = await fetch(`${config.laravelUrl}${PATH}`, {
                method: 'POST',
                body,
                headers: {
                    'content-type': 'application/json',
                    'x-unkvoid-timestamp': String(at),
                    'x-unkvoid-signature': Signature.header(String(at), 'POST', PATH, body),
                },
                signal: AbortSignal.timeout(3000),
            });
        } catch (failure) {
            // Recusar por causa de uma falha de rede esconderia o chat de quem tem direito
            // a ele; dizer que o Laravel não respondeu deixa o app tentar de novo.
            throw new ServiceUnavailableException(`authorization failed: ${String(failure)}`);
        }

        if (response.status === 403 || response.status === 404) {
            throw new ForbiddenException('not authorized for this channel');
        }

        if (!response.ok) {
            throw new ServiceUnavailableException(`authorization answered ${response.status}`);
        }

        const answer = (await response.json()) as Answer;

        if (answer.allowed !== true) {
            throw new ForbiddenException('not authorized for this channel');
        }

        return typeof answer.name === 'string' ? answer.name : null;
    }
}
