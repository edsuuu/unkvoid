import { beforeAll, describe, expect, it } from 'vitest';

import { App } from '../../ui/core/App.ts';
import { Broadcast } from '../../ui/core/Broadcast.ts';
import { Sharing } from '../../ui/core/Sharing.ts';

type RequestData = { kind?: string; producerId?: string; source?: string };

describe('transmissão: a ordem que o Rust e o servidor precisam', () => {
    const calls: string[] = [];
    const callArgs = new Map<string, unknown>();
    const offers: unknown[] = [];
    const requests: string[] = [];
    const sfu = {
        tolerate(action: string, data: RequestData) {
            return this.request(action, data).catch(() => null);
        },
        request: async (action: string, data: RequestData) => {
            requests.push(`${action}:${data.kind ?? data.producerId ?? ''}/${data.source ?? ''}`);

            return { producerId: data.kind ? `${data.kind}-producer` : undefined, ip: '10.0.0.1', port: 41000 };
        },
    };
    let broadcast: Broadcast;

    beforeAll(() => {
        window.__TAURI__ = {
            core: {
                invoke: async (command, args) => {
                    calls.push(command);
                    callArgs.set(command, args);

                    if (command === 'sfu_offer') {
                        offers.push(args?.source);

                        return { rtpParameters: { codecs: [] }, srtpParameters: { keyBase64: 'k' } };
                    }

                    return command === 'stop_broadcast' ? 4242 : null;
                },
            },
            event: { listen: async () => () => null },
        };
        broadcast = new Broadcast(sfu as never);
    });

    it('o use_sfu vem depois de declarar vídeo e áudio: antes, o servidor descarta calado o RTP de um SSRC que não conhece', async () => {
        await broadcast.start('1080', 30, 'window:87', true, true);

        expect(calls).toEqual(['start_broadcast', 'sfu_offer', 'sfu_offer', 'use_sfu']);
        expect(calls.indexOf('use_sfu'), 'use_sfu é o último').toBe(calls.length - 1);
    });

    it('qualidade, fps e as duas opções de áudio chegam inteiras ao Rust', () => {
        expect(callArgs.get('start_broadcast')).toEqual({ quality: '1080', fps: 30, source: 'window:87', audio: true, muteCalls: true });
    });

    it('declara a tela e o áudio da tela, pedindo a oferta ao Rust pela origem, e não pelo tipo', () => {
        expect(requests).toEqual(['producePlain:video/screen', 'producePlain:audio/screenAudio']);
        expect(offers).toEqual(['screen', 'screenAudio']);
        expect(broadcast.broadcasting).toBe(true);
    });

    it('com o servidor reiniciado, republicar troca a chave antes das ofertas e não reinicia a captura', async () => {
        const before = calls.length;

        expect(await broadcast.republish()).toBe(true);
        expect(calls.slice(before)).toEqual(['renew_sfu_key', 'sfu_offer', 'sfu_offer', 'use_sfu']);
        expect(broadcast.producerIds, 'a lista de producers não acumula').toEqual(['video-producer', 'audio-producer']);
        expect(broadcast.videoProducerId).toBe('video-producer');
    });

    it('parar fecha os producers no servidor e devolve os quadros transmitidos', async () => {
        expect(await broadcast.stop(), 'stop devolve os quadros transmitidos').toBe(4242);
        expect(broadcast.broadcasting).toBe(false);
        expect(requests).toEqual([
            'producePlain:video/screen',
            'producePlain:audio/screenAudio',
            'producePlain:video/screen',
            'producePlain:audio/screenAudio',
            'closeProducer:video-producer/',
            'closeProducer:audio-producer/',
        ]);
    });

    it('republicar sem transmissão nenhuma não fala com o Rust', async () => {
        expect(await new Broadcast(sfu as never).republish()).toBe(false);
    });

    it('parar duas vezes não manda um segundo stop_broadcast', async () => {
        const before = calls.length;

        expect(await broadcast.stop()).toBe(0);
        expect(calls.length, 'parar de novo não fala com o Rust').toBe(before);
    });

    it('sem o áudio marcado só a tela é declarada: um áudio que nunca sai morreria aos 30 s e levaria a tela junto', async () => {
        const silent = new Broadcast(sfu as never);

        requests.length = 0;
        await silent.start('720', 30, 'display:1', false, false);
        expect(requests).toEqual(['producePlain:video/screen']);

        requests.length = 0;
        expect(await silent.republish()).toBe(true);
        expect(requests, 'republicar também não declara o áudio').toEqual(['producePlain:video/screen']);

        await silent.stop();
    });

    it('uma falha depois de iniciar a captura libera o estado nativo para a próxima tentativa', async () => {
        const failingSfu = {
            tolerate: sfu.tolerate,
            request: async (action: string, data: RequestData) => {
                if (action === 'producePlain' && data.kind === 'audio') {
                    throw new Error('SFU indisponível');
                }

                return { producerId: 'partial-producer', ip: '10.0.0.1', port: 41000 };
            },
        };
        const partial = new Broadcast(failingSfu as never);

        await expect(partial.start('1080', 30, 'display:1', true, false)).rejects.toThrow(/SFU indisponível/);
        expect(partial.nativeActive).toBe(false);
        expect(partial.producerIds).toEqual([]);
    });
});

describe('transmissão: a linha de números', () => {
    const reading = (sent: number, extra: Record<string, unknown> = {}) => ({ active: true, captured: sent, sent, sentBytes: sent * 10_000, sendDropped: 2, encodeErrors: 0, sendErrors: 0, audioErrors: 0, encoder: 'gpu', ...extra });

    beforeAll(() => {
        window.__TAURI__ = { core: { invoke: async () => null }, event: { listen: async () => () => null } };
    });

    it('mostra a perda em %, e só fala em internet apertada quando o alvo cai abaixo do teto com que a qualidade nasceu', async () => {
        const sharing = new App().sharing;

        sharing.updateStats(reading(0, { targetBitrate: 8_000_000, lossPermille: 0 }));
        expect(Sharing.numbers(sharing.store.state.line), 'a primeira leitura ainda não tem taxa').toBe('');

        sharing.updateStats(reading(60, { targetBitrate: 8_000_000, lossPermille: 4 }));
        expect(Sharing.numbers(sharing.store.state.line)).toMatch(/^\d+ fps · \d+\.\d Mb\/s · 2 perdidos · perda 0\.4%$/);

        sharing.updateStats(reading(120, { targetBitrate: 3_600_000, lossPermille: 62 }));
        expect(Sharing.numbers(sharing.store.state.line)).toMatch(/perda 6\.2% · internet apertada: reduzido para 3\.6 Mb\/s$/);

        sharing.updateStats(reading(180, { targetBitrate: 8_000_000, lossPermille: 0 }));
        expect(Sharing.numbers(sharing.store.state.line), 'o alvo voltou ao teto: o aviso some').not.toMatch(/apertada/);

        sharing.store.set({ active: true });
        sharing.app.media.broadcast = { changeQuality: async () => undefined } as never;
        await sharing.changeQuality('720', '30');
        sharing.updateStats(reading(240, { targetBitrate: 4_000_000, lossPermille: 0 }));
        expect(Sharing.numbers(sharing.store.state.line), 'qualidade menor tem teto menor: não é internet apertada').not.toMatch(/apertada/);
    });

    it('Rust antigo e ponte fingida sem os dois campos: a linha sai como sempre, sem perda e sem aviso', () => {
        const sharing = new App().sharing;

        sharing.updateStats(reading(0));
        sharing.updateStats(reading(60, { targetBitrate: 'muito', lossPermille: null }));

        expect(sharing.store.state.line).toMatchObject({ lossPercent: null, reducedToMbps: null });
        expect(Sharing.numbers(sharing.store.state.line)).toMatch(/^\d+ fps · \d+\.\d Mb\/s · 2 perdidos$/);
    });
});
