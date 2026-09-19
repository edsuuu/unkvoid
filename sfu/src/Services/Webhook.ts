import { config } from '../config.js';
import type { Peer } from './Peer.js';
import { Signature } from './Signature.js';

const PATH = '/api/sfu/events';

/**
 * Avisa o Laravel de quem entrou e saiu de um canal.
 * Fora do caminho do `join` de propósito: o site fora do ar não pode impedir ninguém de
 * falar, então isto nunca espera resposta nem lança — no máximo reclama no log.
 */
export class Webhook {
    /**
     * Conta e visitante. O visitante da sala por código não tem canal a avisar, mas entra
     * na auditoria com nome e IP. Qualquer outro `sub` o Laravel recusaria.
     */
    public static send(event: 'joined' | 'left', roomId: string, peer: Peer): void {
        if (!peer.userId.startsWith('user:') && !peer.userId.startsWith('guest:')) {
            return;
        }

        Webhook.post(event, { room: roomId, sub: peer.userId, name: peer.name, ip: peer.ip });
    }

    private static post(event: string, data: Record<string, unknown>): void {
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
