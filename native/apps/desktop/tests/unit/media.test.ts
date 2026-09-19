import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { App } from '../../ui/core/App.ts';
import { Media } from '../../ui/core/Media.ts';
import { SfuClient } from '../../ui/core/SfuClient.ts';

const SOURCES: Record<string, string> = { 'a-screen': 'screenAudio', 'a-mic': 'mic', 'v-camera': 'camera', 'v-screen': 'screen' };

describe('o palco: o que cada origem vira e o que custa decoder', () => {
    const track = { stop() {} };
    const closed: string[] = [];
    const pausedCalls: string[] = [];
    const toasts: string[] = [];
    const failures: string[] = [];
    let app: App;
    let media: Media;

    beforeAll(async () => {
        window.__TAURI__ = { core: { invoke: async () => null }, event: { listen: async () => () => null } };
        window.HTMLMediaElement.prototype.play = async () => undefined;
        window.HTMLMediaElement.prototype.pause = () => undefined;
        vi.stubGlobal('MediaStream', class {
            tracks: unknown[];

            constructor(tracks: unknown[] = []) {
                this.tracks = tracks;
            }

            getTracks() {
                return this.tracks;
            }
        });

        Object.defineProperty(navigator, 'platform', { value: 'Win32', configurable: true });

        app = new App();
        media = app.media;
        media.sfu = {
            peerId: 'me',
            canWatch: () => true,
            consumersHasProducer: () => false,
            consumersOf: () => [],
            consumerPeers: new Map(),
            consumers: new Map(),
            peers: new Map([['ana', { peerId: 'ana', name: 'Ana', producers: [] }]]),
            consume: async (producerId: string) => {
                const created = { id: `c-${producerId}`, track, kind: producerId.startsWith('a') ? 'audio' : 'video' };

                media.sfu.consumerPeers.set(created.id, 'ana');
                media.sfu.consumers.set(created.id, created);

                return { consumer: created, peerId: 'ana', source: SOURCES[producerId] };
            },
            closeConsumer: async (consumerId: string) => closed.push(consumerId),
            setPeerPaused: async (owner: string, value: boolean, kind: string) => pausedCalls.push(`${owner}:${value}:${kind}`),
        };

        for (const producerId of Object.keys(SOURCES)) {
            await media.consume({ producerId, peerId: 'ana', kind: producerId.startsWith('a') ? 'audio' : 'video', source: SOURCES[producerId] });
        }
    });

    afterAll(() => {
        for (const timer of media.mediaStatsTimers.values()) {
            clearInterval(timer);
        }
    });

    it('o áudio da tela chega mudo e em zero, o mic toca direto e a câmera vira cartão', () => {
        expect(media.remoteAudios.get('ana')?.muted, 'áudio da tela chega mudo').toBe(true);
        expect(media.store.state.audio.ana, 'e a interface vê mudo e em zero').toEqual({ volume: 0, muted: true });
        expect(media.micAudios.get('a-mic')?.muted, 'mic toca direto').toBe(false);
        expect(media.tile('ana/camera')?.kind, 'câmera vira cartão').toBe('camera');
        expect(media.tile('ana')?.kind).toBe('screen');
    });

    it('desmutar o áudio da tela com o volume em zero sobe o volume: quem clicou quer ouvir', () => {
        media.toggleAudioMute('ana');

        expect(media.store.state.audio.ana).toEqual({ volume: 100, muted: false });
    });

    it('tela e câmera têm ajustes próprios, e os dois sobrevivem a fechar o app', () => {
        media.setImage('screen', 'contrast', 150);
        expect(media.store.state.image.screen.contrast).toBe(150);
        expect(localStorage.getItem('unkvoid.contraste')).toBe('150');
        expect(media.store.state.image.camera.contrast, 'mexer na tela não mexe na câmera').toBe(100);

        media.setImage('camera', 'saturation', 70);
        media.setImage('camera', 'blur', 6);
        expect(media.store.state.image.camera.saturation).toBe(70);
        expect(media.store.state.image.camera.blur, 'o desfoque da câmera').toBe(6);
        expect(localStorage.getItem('unkvoid.saturacao.camera')).toBe('70');
        expect(media.store.state.image.screen.saturation, 'e a câmera não mexe na tela').toBe(100);

        media.resetImage('camera');
        expect(media.store.state.image.camera.saturation, 'voltar ao padrão').toBe(100);
        expect(media.store.state.image.camera.blur, 'desfoque volta a zero').toBe(0);
        expect(localStorage.getItem('unkvoid.saturacao.camera')).toBeNull();
        expect(media.store.state.image.screen.contrast, 'sem levar o ajuste da tela junto').toBe(150);
    });

    it('pausar pede ao servidor só o vídeo, e a interface vê a tela pausada', async () => {
        await media.togglePause('ana');
        expect(pausedCalls).toEqual(['ana:true:video']);
        expect(media.store.state.paused).toEqual(['ana']);

        await media.togglePause('ana');
        expect(media.store.state.paused).toEqual([]);
    });

    it('fechar a tela de alguém fecha a tela e o áudio dela, e nunca o microfone', async () => {
        await media.closeTile('ana');

        expect(closed).toContain('c-v-screen');
        expect(closed).toContain('c-a-screen');
        expect(closed, 'o mic segue tocando').not.toContain('c-a-mic');
        expect(media.tile('ana')).toBeNull();
        expect(media.remoteAudios.has('ana')).toBe(false);
        expect(media.micAudios.has('a-mic')).toBe(true);
    });

    it('transmitindo e sem cartão na tela, o Assistir acende', () => {
        media.sfu!.peers.get('ana')!.sharing = true;
        media.refreshPeople();

        expect(media.store.state.pending).toBe(true);
        expect(media.store.state.peers.find(peer => peer.peerId === 'ana')?.missing).toBe(true);
    });

    it('o mic que fechou do outro lado tira o <audio> do documento', () => {
        media.forgetProducer({ producerId: 'a-mic', peerId: 'ana', kind: 'audio', source: 'mic' });

        expect(media.micAudios.size).toBe(0);
        expect(document.querySelectorAll('audio[data-remote="ana"]').length).toBe(0);
    });

    it('acima do teto de telas o cartão espera o Assistir, e o Assistir passa por cima do teto', async () => {
        expect([2, 4]).toContain(Media.MAX_SCREENS);

        const crowd = ['bia', 'caio'].map(owner => ({ peerId: owner, name: owner, sharing: true, producers: [{ producerId: `v-${owner}`, kind: 'video', source: 'screen' }] }));

        crowd.forEach(peer => media.sfu!.peers.set(peer.peerId, peer));
        media.sfu!.consume = async (producerId: string) => ({ consumer: { id: `c-${producerId}`, track, kind: 'video' }, peerId: producerId.replace('v-', ''), source: 'screen' });

        await media.consumePeers(crowd, 1);
        expect(media.tile('bia'), 'a primeira tela cabe no teto').toBeTruthy();
        expect(media.tile('caio'), 'acima do teto, o cartão espera o Assistir').toBeNull();

        await media.watchPeer('caio');
        expect(media.tile('caio'), 'clicar em Assistir sempre consome').toBeTruthy();
    });

    it('sem encoder na placa, quem transmite fica sabendo uma vez, e não a cada leitura', () => {
        const cpuReading = { active: true, captured: 0, sent: 0, sentBytes: 0, sendDropped: 0, encodeErrors: 0, sendErrors: 0, audioErrors: 0, busyUs: 0, encoder: 'cpu' };

        app.toast = message => toasts.push(message);

        app.sharing.updateStats({ ...cpuReading, encoder: 'gpu' });
        expect(toasts.length, 'encoder da placa não avisa nada').toBe(0);

        app.sharing.updateStats(cpuReading);
        app.sharing.updateStats(cpuReading);
        expect(toasts.length, 'encoder do processador avisa uma vez').toBe(1);
        expect(toasts[0]).toMatch(/processador/);
    });

    it('quem foi expulso sai com um aviso só, e quem nunca foi visto entrar não vira "alguém saiu"', () => {
        const room = new SfuClient();

        toasts.length = 0;
        media.attachSfu(room);
        room.handleMessage({ event: 'peerJoined', data: { peerId: 'lia', userId: 'user:3', name: 'Lia' } });
        room.handleMessage({ event: 'peerKicked', data: { peerId: 'lia', name: 'Lia' } });
        room.handleMessage({ event: 'peerLeft', data: { peerId: 'lia' } });
        room.handleMessage({ event: 'peerJoined', data: { peerId: 'rui', userId: 'user:4', name: 'Rui' } });
        room.handleMessage({ event: 'peerLeft', data: { peerId: 'rui' } });
        room.handleMessage({ event: 'peerLeft', data: { peerId: 'sessao-velha-da-recarga' } });

        expect(toasts).toEqual(['Lia entrou', 'Lia foi removido', 'Rui entrou', 'Rui saiu']);
    });

    it('a tela e o áudio da tela morrendo juntos avisam uma vez', async () => {
        app.fail = message => failures.push(message);
        media.broadcast = { stop: async () => new Promise(resolve => setTimeout(() => resolve(0), 20)) };
        app.sharing.store.set({ active: true });

        await Promise.all([
            app.sharing.died({ source: 'screen' }),
            app.sharing.died({ source: 'screenAudio' }),
        ]);

        expect(failures.length).toBe(1);
        expect(app.sharing.store.state.active).toBe(false);
    });

    it('só o áudio da tela morrendo não derruba a transmissão: segue sem som, com aviso', async () => {
        const stops: number[] = [];

        failures.length = 0;
        toasts.length = 0;
        media.broadcast = { stop: async () => { stops.push(1); return 0; } };
        app.sharing.store.set({ active: true });

        await app.sharing.died({ source: 'screenAudio' });

        expect(app.sharing.store.state.active, 'a tela continua no ar').toBe(true);
        expect(stops.length, 'nada foi parado').toBe(0);
        expect(failures).toEqual([]);
        expect(toasts.at(-1)).toMatch(/segue sem som/);

        app.sharing.store.set({ active: false });
    });

    it('assistir que o servidor recusa diz por quê', async () => {
        media.sfu = { canWatch: () => true, consumersHasProducer: () => false, consume: async () => { throw new Error('producer not found'); } };

        await media.consume({ producerId: 'v-sumiu', peerId: 'sumiu', kind: 'video', source: 'screen' });

        expect(failures.at(-1)).toBe('não deu para assistir: producer not found');
    });

    it('no Linux a tela que chega com o palco fora de vista já nasce calada no Rust', () => {
        const nativeCalls: { command: string; args: unknown }[] = [];

        media.sfu = {
            peers: new Map([['nina', { peerId: 'nina', name: 'Nina', producers: [{ producerId: 'v-nina', source: 'screen' }] }]]),
            consumersOf: () => [],
            consumerPeers: new Map(),
            consumers: new Map(),
            setPeerPaused: async () => null,
        };
        media.setStageVisible(false);
        window.__TAURI__!.core.invoke = async (command, args) => nativeCalls.push({ command, args });
        media.nativeWatching.set('v-nina', 'nina');
        media.showNativeTile('nina', 'nina', 'v-nina', 'Nina', 5000, 'screen');

        expect(nativeCalls.filter(call => call.command === 'watch_mute').map(call => call.args)).toEqual([{ producerId: 'v-nina', muted: true }]);
    });

    it('o Esc que fechou um menu aberto em tela cheia não sai da tela cheia; o seguinte sai', () => {
        const exits: string[] = [];

        media.toggleFullscreen = async key => {
            exits.push(key);
        };
        media.store.set({ fullscreen: 'nina' });

        app.onKeyDown({ key: 'Escape', defaultPrevented: true });
        expect(exits, 'o Esc que fechou o menu não sai da tela cheia').toEqual([]);

        app.onKeyDown({ key: 'Escape', defaultPrevented: false });
        expect(exits, 'o Esc seguinte sai').toEqual(['nina']);
    });
});
