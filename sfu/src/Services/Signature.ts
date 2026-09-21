import { createHmac, timingSafeEqual } from 'node:crypto';

import { config } from '../Config/index.js';
import { UnauthorizedException, ValidationException } from '../Exceptions/ApiException.js';

export type JoinClaims = {
    room: string;
    sub: string;
    name: string;
    exp: number;

    can: string[];
};

const HEADER_WINDOW_S = 300;

const EXP_LEEWAY_S = 30;

export type SessionClaims = {
    sub: string;
    name: string;
    exp: number;
};

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
     * O token do chat, que não é de sala nenhuma: identifica o socket que fica aberto
     * enquanto o app está logado, para receber mensagem, DM e presença.
     */
    public static sessionClaims(token: string): SessionClaims {
        const claims = Signature.decode<Partial<SessionClaims>>(token);

        if (
            typeof claims.sub !== 'string' ||
            typeof claims.name !== 'string' ||
            typeof claims.exp !== 'number'
        ) {
            throw new ValidationException('field token is missing claims');
        }

        Signature.ensureFresh(claims.exp);

        return claims as SessionClaims;
    }

    private static decode<T>(token: string): T {
        const [body, signature] = token.split('.');

        if (!body || !signature) {
            throw new ValidationException('field token is malformed');
        }

        if (!Signature.equal(signature, Signature.hmac(body))) {
            throw new UnauthorizedException('invalid token signature');
        }

        try {
            return JSON.parse(Buffer.from(body, 'base64url').toString()) as T;
        } catch {
            throw new ValidationException('field token is malformed');
        }
    }

    private static ensureFresh(exp: number): void {
        if ((exp + EXP_LEEWAY_S) * 1000 < Date.now()) {
            throw new UnauthorizedException('token expired');
        }
    }

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
