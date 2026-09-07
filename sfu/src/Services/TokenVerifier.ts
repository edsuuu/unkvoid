import { createHmac, timingSafeEqual } from 'node:crypto';

import { UnauthorizedException } from '../Exceptions/ApiException.js';
import type { TokenClaims } from '../types.js';

export class TokenVerifier {
    constructor(private readonly secret: string) {}

    verify(token: string): TokenClaims {
        if (! this.secret) {
            throw new UnauthorizedException('SFU_SECRET is not configured for the SFU');
        }

        const parts = token.split('.');

        if (parts.length !== 3) {
            throw new UnauthorizedException('token malformado');
        }

        const [header, body, signature] = parts as [string, string, string];
        const expected = createHmac('sha256', this.secret).update(`${header}.${body}`).digest();
        const received = Buffer.from(signature, 'base64url');

        if (received.length !== expected.length || ! timingSafeEqual(received, expected)) {
            throw new UnauthorizedException('invalid signature');
        }

        const claims = JSON.parse(Buffer.from(body, 'base64url').toString('utf8')) as Partial<TokenClaims>;

        if (typeof claims.exp !== 'number' || claims.exp < Math.floor(Date.now() / 1000)) {
            throw new UnauthorizedException('token expirado');
        }

        if (! claims.sub || (! claims.room && ! claims.server)) {
            throw new UnauthorizedException('token has no identity or destination');
        }

        return claims as TokenClaims;
    }
}
