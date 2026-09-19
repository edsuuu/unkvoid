import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { App } from '../../ui/core/App.ts';
import { Mic } from '../../ui/core/Mic.ts';
import type { SfuClient } from '../../ui/core/SfuClient.ts';
import type { Voice } from '../../ui/core/Voice.ts';

function tone(amplitude: number, size = Mic.FFT_SIZE): Float32Array {
    return Float32Array.from({ length: size }, (unused, index) => amplitude * Math.sin((2 * Math.PI * index) / 64));
}

describe('detecção de voz: o nível que decide se o microfone abre', () => {
    it('silêncio fica no chão e volume cheio no teto', () => {
        expect(Mic.levelOf(new Float32Array(Mic.FFT_SIZE))).toBe(0);
        expect(Mic.levelOf(tone(1)), 'seno de amplitude 1 é o mais alto que a placa entrega').toBeGreaterThan(95);
    });

    it('sobe junto com o volume, sem passar do teto nem furar o chão', () => {
        const quiet = Mic.levelOf(tone(0.01));
        const talking = Mic.levelOf(tone(0.2));
        const shouting = Mic.levelOf(tone(0.9));

        expect(quiet).toBeLessThan(talking);
        expect(talking).toBeLessThan(shouting);
        expect(quiet).toBeGreaterThanOrEqual(0);
        expect(shouting).toBeLessThanOrEqual(100);
    });

    it('sussurro fica abaixo do padrão de sensibilidade e fala normal fica acima', () => {
        const DEFAULT_SENSITIVITY = 35;

        expect(Mic.levelOf(tone(0.001)), 'ruído de fundo não pode abrir o microfone').toBeLessThan(DEFAULT_SENSITIVITY);
        expect(Mic.levelOf(tone(0.15)), 'fala normal precisa abrir o microfone').toBeGreaterThan(DEFAULT_SENSITIVITY);
    });
});

describe('detecção de voz: o que o detector escuta', () => {
    it('escuta uma cópia da faixa, que o portão fechado não silencia', () => {
        const probe = { enabled: false, stop: vi.fn() };
        const track = { enabled: false, clone: vi.fn(() => probe) } as unknown as MediaStreamTrack;
        const connected: unknown[] = [];

        vi.stubGlobal('MediaStream', class { constructor(readonly tracks: unknown[]) {} });
        vi.stubGlobal('AudioContext', class {
            state = 'running';
            createAnalyser() {
                return { fftSize: 0, disconnect: vi.fn(), getFloatTimeDomainData: vi.fn() };
            }
            createMediaStreamSource(stream: { tracks: unknown[] }) {
                connected.push(...stream.tracks);

                return { connect: vi.fn(), disconnect: vi.fn() };
            }
            close() {
                return Promise.resolve();
            }
        });

        const mic = new Mic();

        mic.watch(track, 35, () => undefined);

        expect(connected, 'a faixa desligada pelo portão só entrega silêncio, e o portão nunca abriria').toEqual([probe]);
        expect(probe.enabled).toBe(true);

        mic.stop();
        vi.unstubAllGlobals();

        expect(probe.stop).toHaveBeenCalled();
    });
});

describe('detecção de voz no Linux: o nível vem do Rust, e a régua é a mesma do AnalyserNode', () => {
    afterAll(() => {
        vi.useRealTimers();
    });

    it('o RMS que o Rust manda cai no mesmo ponto da escala que as amostras da janela', () => {
        for (const amplitude of [0.001, 0.05, 0.2, 0.9]) {
            expect(Math.abs(Mic.levelOfRms(amplitude / Math.SQRT2) - Mic.levelOf(tone(amplitude))), `amplitude ${amplitude}`).toBeLessThanOrEqual(1);
        }

        expect(Mic.levelOfRms(0)).toBe(0);
        expect(Mic.levelOfRms(1)).toBe(100);
    });

    it('mesmo limiar e mesma cauda de 350 ms; o primeiro nível decide o portão, e nível parado abre o microfone em vez de calar', () => {
        const gate: boolean[] = [];
        const problems: string[] = [];
        const mic = new Mic();

        vi.useFakeTimers();
        vi.setSystemTime(1_000_000);
        mic.onError(stage => problems.push(stage));

        mic.feed(0.5);
        expect(gate, 'sem ninguém ouvindo o nível, nada acontece').toEqual([]);

        mic.watchLevels(35, speaking => gate.push(speaking));
        mic.feed(0.0005);
        expect(gate, 'o primeiro nível fecha o portão mesmo sem mudança de estado').toEqual([false]);

        mic.feed(0.2);
        expect(gate).toEqual([false, true]);

        vi.advanceTimersByTime(300);
        mic.feed(0.0005);
        expect(gate, 'dentro da cauda segue aberto').toEqual([false, true]);

        vi.advanceTimersByTime(100);
        mic.feed(0.0005);
        expect(gate, 'passada a cauda, fecha').toEqual([false, true, false]);

        mic.setThreshold(90);
        mic.feed(0.2);
        expect(gate, 'a sensibilidade nova vale na hora').toEqual([false, true, false]);

        vi.advanceTimersByTime(Mic.STALL_MS + 600);
        expect(gate.at(-1), 'o Rust parou de mandar nível: o microfone abre em vez de calar a pessoa').toBe(true);
        expect(problems).toEqual(['level']);

        mic.stop();
        vi.useRealTimers();
    });
});

describe('a voz quando o servidor fecha um producer por conta própria', () => {
    const CHANNEL = { id: 'voice-1', name: 'Geral', type: 'voice', permissions: 0 };
    const steps: string[] = [];
    const toasts: string[] = [];
    const failures: string[] = [];
    const heard: Record<string, (event: { payload: unknown }) => void> = {};
    let grants: string[] = [];
    let app: App;
    let voice: Voice;
    let room: SfuClient;
    let micTrack: { enabled: boolean; stopped: boolean; stop(): void };

    const settle = () => new Promise(resolve => setTimeout(resolve, 0));

    const enter = async (native: boolean, inputMode = 'open') => {
        app = new App();
        voice = app.hub.voice;
        voice.native = () => native;
        app.toast = message => toasts.push(message);
        app.fail = message => failures.push(message);
        app.hub.user = { id: 1, name: 'Edsu' };
        app.hub.api.request = async (method: string, path: string) => {
            steps.push(`${method} ${path}`);

            return { token: 'token-novo' };
        };
        app.media.tearDown = async () => {
            app.media.sfu = null;
        };
        app.media.enterRoom = async (sfu, identity, alongside) => {
            room = sfu;
            sfu.peerId = 'me';
            sfu.request = async (action: string) => {
                steps.push(`request:${action}`);

                return { producerId: 'mic-native', ip: '127.0.0.1', port: 40000 };
            };
            sfu.produce = async (_track: unknown, source: string) => {
                const producer = { id: `${source}-1`, close: () => steps.push(`local-close:${source}-1`), pause() {}, resume() {} };

                sfu.producers.set(producer.id, producer);
                steps.push(`produce:${source}`);

                return producer;
            };
            app.media.attachSfu(sfu);
            await identity();
            await alongside({ can: grants });
        };

        await voice.setPreference('muteOnJoin', false);
        await voice.setPreference('inputMode', inputMode);
        await voice.join(CHANNEL);
    };

    beforeAll(() => {
        window.__TAURI__ = {
            core: {
                invoke: async (command, args) => {
                    if (command !== 'log_line') {
                        steps.push(command === 'set_voice_muted' ? `invoke:set_voice_muted:${args?.muted}` : `invoke:${command}`);
                    }

                    return command === 'sfu_offer' ? { ssrc: 7 } : null;
                },
            },
            event: {
                listen: async (event, handler) => {
                    heard[event] = handler;

                    return () => null;
                },
            },
        };
        vi.stubGlobal('MediaStream', class {
            tracks: unknown[];

            constructor(tracks: unknown[] = []) {
                this.tracks = tracks;
            }

            getTracks() {
                return this.tracks;
            }
        });
    });

    beforeEach(() => {
        steps.length = 0;
        toasts.length = 0;
        failures.length = 0;
        grants = ['speak', 'video', 'stream'];
        micTrack = { enabled: true, stopped: false, stop() { this.stopped = true; } };
        Object.defineProperty(window.navigator, 'mediaDevices', {
            value: {
                getUserMedia: async ({ video }: { video?: unknown }) => ({
                    getAudioTracks: () => [micTrack],
                    getVideoTracks: () => [{ enabled: Boolean(video), stop() {} }],
                }),
            },
            configurable: true,
        });
    });

    afterAll(() => {
        localStorage.clear();
        vi.unstubAllGlobals();
    });

    it('retomada sem speak e sem video: o mic e a câmera saem sem closeProducer, e os botões seguem o can novo', async () => {
        await enter(false);
        await voice.toggleCamera();
        expect(voice.store.state.cameraOn).toBe(true);
        steps.length = 0;

        room.emit('reconnected', { resumed: true, peers: [], can: ['stream'] });
        await settle();

        expect(voice.micProducerId).toBeNull();
        expect(voice.micTrack, 'a trilha do getUserMedia é largada').toBeNull();
        expect(micTrack.stopped).toBe(true);
        expect(voice.cameraProducerId).toBeNull();
        expect(steps, 'o producer local fecha, e nada é pedido ao servidor: lá ele já fechou e daria 404').toEqual(['local-close:mic-1', 'local-close:camera-1']);
        expect(voice.store.state.can, 'mic e câmera cinzas, tela ainda permitida').toEqual(['stream']);
        expect(voice.store.state.cameraOn).toBe(false);
        expect(app.media.tile('me/camera'), 'o cartão da própria câmera sai').toBeNull();
        expect(toasts.filter(message => /perdeu a permissão/.test(message)).length).toBe(2);
    });

    it('o speak que volta depois disso sai e entra com token novo, sem resumeProducer num producer que morreu', async () => {
        await enter(false);
        room.emit('serverMuted', { muted: true });
        room.emit('reconnected', { resumed: true, peers: [], can: [] });
        await settle();
        expect(voice.micProducerId).toBeNull();
        steps.length = 0;

        room.emit('serverMuted', { muted: false });
        await settle();
        await voice.leaving;
        await settle();

        expect(steps.filter(step => step.startsWith('request:')), 'nenhum resumeProducer').toEqual([]);
        expect(steps).toContain('POST /api/channels/voice-1/voice/token');
        expect(steps).toContain('produce:mic');
        expect(voice.micProducerId).toBe('mic-1');
        expect(voice.store.state.can).toContain('speak');
        expect(voice.store.state.serverMuted).toBe(false);
    });

    it('no Linux a retomada sem speak para a captura do Rust, e não fala closeProducer', async () => {
        await enter(true);
        expect(steps).toContain('invoke:start_voice');
        expect(voice.micProducerId).toBe('mic-native');
        steps.length = 0;

        room.emit('reconnected', { resumed: true, peers: [], can: ['stream', 'video'] });
        await settle();

        expect(voice.micProducerId).toBeNull();
        expect(steps).toContain('invoke:stop_voice');
        expect(steps).not.toContain('request:closeProducer');
    });

    it('sem stream na volta a tela sai do ar pela permissão, com ou sem o producerDead, e ninguém tenta republicar', async () => {
        const broadcast = { stop: async () => 0, republish: async () => { steps.push('republish'); return true; }, videoProducerId: null };

        await enter(false);
        app.media.broadcast = broadcast;
        app.sharing.store.set({ active: true });

        room.emit('reconnected', { resumed: true, peers: [], can: ['speak', 'video'] });
        await settle();

        expect(app.sharing.store.state.active).toBe(false);
        expect(failures).toEqual(['você perdeu a permissão de transmitir neste canal: a transmissão foi encerrada.']);
        expect(voice.micProducerId, 'o mic continua: speak ficou').toBe('mic-1');

        failures.length = 0;
        app.media.broadcast = broadcast;
        app.sharing.store.set({ active: true });

        room.emit('reconnected', { resumed: false, peers: [], can: ['speak', 'video'] });
        await settle();
        await settle();

        expect(steps, 'sessão nova sem stream: republicar levaria 403').not.toContain('republish');
        expect(failures.length).toBe(1);
        expect(failures[0]).toMatch(/permissão de transmitir/);
    });

    it('o mic que o relógio de 30 s matou no servidor é largado, e o clique seguinte publica de novo', async () => {
        await enter(false);
        steps.length = 0;

        room.emit('producerDead', { producerId: 'de-outra-sessao', kind: 'audio', source: 'mic' });
        await settle();
        expect(voice.micProducerId, 'producer que não é o meu não mexe em nada').toBe('mic-1');

        room.emit('producerDead', { producerId: 'mic-1', kind: 'audio', source: 'mic' });
        await settle();

        expect(voice.micProducerId).toBeNull();
        expect(voice.store.state.muted, 'ninguém está ouvindo: o botão mostra mutado').toBe(true);
        expect(steps).not.toContain('request:closeProducer');
        expect(toasts.at(-1)).toMatch(/não chegou ao servidor/);

        steps.length = 0;
        await voice.toggleMute();

        expect(steps, 'publica de novo em vez de retomar o que não existe').toContain('produce:mic');
        expect(voice.micProducerId).toBe('mic-1');
        expect(voice.store.state.muted).toBe(false);
    });

    it('no Linux a detecção de voz abre e fecha o mic do Rust pelo voice:level, e sem evento nenhum ele fica aberto como antes', async () => {
        await enter(true, 'voice');
        voice.listenNative();
        await settle();

        expect(steps.filter(step => step.startsWith('invoke:set_voice_muted')).at(-1), 'sem nível chegando, aberto').toBe('invoke:set_voice_muted:false');

        heard['voice:level']({ payload: { level: 0.0005 } });
        expect(steps.at(-1), 'o primeiro nível baixo fecha o portão').toBe('invoke:set_voice_muted:true');
        expect(voice.store.state.speaking).toBe(false);

        heard['voice:level']({ payload: { level: 0.2 } });
        expect(steps.at(-1)).toBe('invoke:set_voice_muted:false');
        expect(voice.store.state.speaking).toBe(true);
        expect(voice.mic.store.state.level, 'o medidor das configurações anda junto').toBeGreaterThan(35);

        await voice.leave();
        steps.length = 0;
        heard['voice:level']({ payload: { level: 0.2 } });
        expect(steps, 'nível que chega fora da voz não mexe em nada').toEqual([]);
    });
});
