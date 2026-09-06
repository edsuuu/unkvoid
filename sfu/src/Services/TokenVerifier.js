import { createHmac, timingSafeEqual } from 'node:crypto';

import { UnauthorizedException } from '../Exceptions/ApiException.js';

export class TokenVerifier {
    constructor(secret) {
        this.secret = secret;
    }

    verify(token) {
        if (! this.secret) {
            throw new UnauthorizedException('o SFU está sem SFU_SECRET configurado');
        }

        const parts = token.split('.');

        if (parts.length !== 3) {
            throw new UnauthorizedException('token malformado');
        }

        const [header, body, signature] = parts;
        const expected = createHmac('sha256', this.secret).update(`${header}.${body}`).digest();
        const received = Buffer.from(signature, 'base64url');

        if (received.length !== expected.length || ! timingSafeEqual(received, expected)) {
            throw new UnauthorizedException('assinatura inválida');
        }

        const claims = JSON.parse(Buffer.from(body, 'base64url').toString('utf8'));

        if (typeof claims.exp !== 'number' || claims.exp < Math.floor(Date.now() / 1000)) {
            throw new UnauthorizedException('token expirado');
        }

        if (! claims.sub || ! claims.room) {
            throw new UnauthorizedException('token sem identidade ou sala');
        }

        return claims;
    }
}
