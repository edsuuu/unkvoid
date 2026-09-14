import { createHmac, timingSafeEqual } from 'node:crypto';

import { config } from '../config.js';
import { UnauthorizedException, ValidationException } from '../Exceptions/ApiException.js';

/** O que o Laravel afirma sobre quem está entrando. Ele decide; o SFU só confere a assinatura. */
export type JoinClaims = {
    room: string;
    sub: string;
    name: string;
    exp: number;
    /** `speak`, `stream`, `video`: o que a pessoa pode produzir. */
    can: string[];
};

/** Quanto o relógio de quem chama pode discordar do nosso antes de a assinatura ser recusada. */
const HEADER_WINDOW_S = 300;

/** Folga no vencimento do token: o relógio do app e o do site nunca batem exatamente. */
const EXP_LEEWAY_S = 30;

/**
 * Tudo o que é assinado entre o Laravel e o SFU passa por aqui, com o mesmo segredo dos
 * dois lados. Duas formas: o token que o cliente apresenta no `join`, e o cabeçalho das
 * chamadas HTTP que o Laravel faz direto ao SFU.
 *
 * HMAC e não JWT de propósito: são dois processos nossos com um segredo compartilhado,
 * e a biblioteca de JWT só traria algoritmos a mais para acertar `none`.
 */
export class Signature {
    public static token(claims: JoinClaims): string {
        const body = Buffer.from(JSON.stringify(claims)).toString('base64url');

        return `${body}.${Signature.hmac(body)}`;
    }

    public static claims(token: string): JoinClaims {
        const [body, signature] = token.split('.');

        if (!body || !signature) {
            throw new ValidationException('field token is malformed');
        }

        if (!Signature.equal(signature, Signature.hmac(body))) {
            throw new UnauthorizedException('invalid token signature');
        }

        let claims: Partial<JoinClaims>;

        try {
            claims = JSON.parse(Buffer.from(body, 'base64url').toString()) as Partial<JoinClaims>;
        } catch {
            throw new ValidationException('field token is malformed');
        }

        if (
            typeof claims.room !== 'string' ||
            typeof claims.sub !== 'string' ||
            typeof claims.name !== 'string' ||
            typeof claims.exp !== 'number' ||
            !Array.isArray(claims.can) ||
            !claims.can.every((entry) => typeof entry === 'string')
        ) {
            throw new ValidationException('field token is missing claims');
        }

        if ((claims.exp + EXP_LEEWAY_S) * 1000 < Date.now()) {
            throw new UnauthorizedException('token expired');
        }

        return claims as JoinClaims;
    }

    /**
     * O cabeçalho cobre método, caminho, hora e corpo: mudar qualquer um invalida a
     * assinatura, e a hora fecha a janela de repetição.
     */
    public static header(timestamp: string, method: string, path: string, body: string): string {
        return Signature.hmac(`${timestamp}\n${method.toUpperCase()}\n${path}\n${body}`);
    }

    public static verifyHeader(
        timestamp: string | undefined,
        signature: string | undefined,
        method: string,
        path: string,
        body: string,
    ): void {
        if (!timestamp || !signature) {
            throw new UnauthorizedException('missing signature headers');
        }

        const skew = Math.abs(Date.now() / 1000 - Number(timestamp));

        if (!Number.isFinite(skew) || skew > HEADER_WINDOW_S) {
            throw new UnauthorizedException('signature timestamp out of window');
        }

        if (!Signature.equal(signature, Signature.header(timestamp, method, path, body))) {
            throw new UnauthorizedException('invalid signature');
        }
    }

    private static hmac(input: string): string {
        return createHmac('sha256', config.secret).update(input).digest('hex');
    }

    private static equal(given: string, expected: string): boolean {
        return (
            given.length === expected.length &&
            timingSafeEqual(Buffer.from(given), Buffer.from(expected))
        );
    }
}
