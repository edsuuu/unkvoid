import { readdir, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const UI = resolve('ui');
const GLOBALS = /(?<!\.)\b(RTCRtpReceiver|RTCRtpSender|RTCPeerConnection)\b/g;
const SAFE = /(typeof\s+|globalThis\.)$/;

describe('WebRTC ausente: no WebKitGTK citar o que não existe derruba o app', () => {
    it('nenhum global de WebRTC aparece cru na interface, só com typeof ou globalThis.', async () => {
        const files = (await readdir(UI, { recursive: true })).filter(name => /\.tsx?$/.test(name) && ! name.startsWith('dev'));
        const bare: string[] = [];

        for (const name of files) {
            const source = await readFile(join(UI, name), 'utf8');

            for (const hit of source.matchAll(GLOBALS)) {
                if (SAFE.test(source.slice(0, hit.index))) {
                    continue;
                }

                bare.push(`${name}:${source.slice(0, hit.index).split('\n').length} ${hit[1]}`);
            }
        }

        expect(bare, `global de WebRTC citado cru (use typeof ou globalThis.): ${bare.join(', ')}`).toEqual([]);
    });

    it('a forma segura devolve nulo em vez de levantar', () => {
        expect(globalThis.RTCRtpReceiver?.getCapabilities?.('video')?.codecs ?? null).toBeNull();
    });
});
