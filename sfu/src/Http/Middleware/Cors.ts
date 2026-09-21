import type { NextFunction, Request, Response } from 'express';

import { config } from '../../Config/index.js';

const allowsEveryone = config.corsOrigins.includes('*');

export const cors = (request: Request, response: Response, next: NextFunction): void => {
    const origin = request.header('origin');

    if (allowsEveryone) {
        response.setHeader('Access-Control-Allow-Origin', '*');
    } else if (origin && config.corsOrigins.includes(origin)) {
        // Devolve a origem que pediu, e não a lista: o cabeçalho só aceita uma. O `Vary`
        // impede que um proxy sirva a resposta de uma origem para outra.
        response.setHeader('Access-Control-Allow-Origin', origin);
        response.setHeader('Vary', 'Origin');
    }

    response.setHeader(
        'Access-Control-Allow-Headers',
        'content-type, x-unkvoid-timestamp, x-unkvoid-signature',
    );

    response.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');

    if (request.method === 'OPTIONS') {
        response.status(204).end();

        return;
    }

    next();
};
