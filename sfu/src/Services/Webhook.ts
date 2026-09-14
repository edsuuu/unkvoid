import { config } from '../config.js';
import type { Peer } from './Peer.js';
import { Signature } from './Signature.js';

const PATH = '/api/sfu/events';

/**
 * Avisa o Laravel de quem entrou e saiu de um canal, e de quando um clipe fica pronto.
 * Fora do caminho do `join` de propósito: o site fora do ar não pode impedir ninguém de
 * falar, então isto nunca espera resposta nem lança — no máximo reclama no log.
 */
export class Webhook {
    public static send(event: 'joined' | 'left', roomId: string, peer: Peer): void {
        // A sala anônima entra como `guest:`: não há conta para o Laravel anotar.
        if (!peer.userId.startsWith('user:')) {
            return;
        }

        Webhook.post(event, { room: roomId, sub: peer.userId, name: peer.name, ip: peer.ip });
    }

    public static post(event: string, data: Record<string, unknown>): void {
        if (config.laravelUrl === '') {
            return;
        }

        const at = Math.floor(Date.now() / 1000);
        const body = JSON.stringify({ event, ...data, at });

        void (async () => {
            try {
                const response = await fetch(`${config.laravelUrl}${PATH}`, {
                    method: 'POST',
                    body,
                    headers: {
                        'content-type': 'application/json',
                        'x-unkvoid-timestamp': String(at),
                        'x-unkvoid-signature': Signature.header(String(at), 'POST', PATH, body),
                    },
                    signal: AbortSignal.timeout(3000),
                });

                if (!response.ok) {
                    console.warn(`[WARN] webhook ${event} answered ${response.status}`);
                }
            } catch (failure) {
                console.warn(`[WARN] webhook ${event} failed: ${String(failure)}`);
            }
        })();
    }
}
