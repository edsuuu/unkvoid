import type { NextFunction, Request as ExpressRequest, Response } from 'express';

import { Signature } from '../../Services/Signature.js';

export type SignedRequest = ExpressRequest & { rawBody?: string };

export const verifySignature = (
    request: SignedRequest,
    _response: Response,
    next: NextFunction,
): void => {
    Signature.verifyHeader(
        request.header('x-unkvoid-timestamp'),
        request.header('x-unkvoid-signature'),
        request.method,
        request.originalUrl.split('?')[0] ?? '',
        request.rawBody ?? '',
    );

    next();
};
