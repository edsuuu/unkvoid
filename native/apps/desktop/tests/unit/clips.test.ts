import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { App } from '../../ui/core/App.ts';
import { Clips } from '../../ui/core/Clips.ts';
import type { Hub } from '../../ui/core/Hub.ts';

const DAY = 86_400_000;

describe('a aba Clipes e o Clipar, com a API de mentira', () => {
    const invokes: { command: string; args: unknown }[] = [];
    const toasts: string[] = [];
    const calls: { method: string; path: string; body: unknown }[] = [];
    const responses = new Map<string, unknown>();
    const now = Date.now();
    let app: App;
    let hub: Hub;

    const clip = (id: string, status: string, extra: object = {}) => ({
        id,
        status,
        streamer: { id: 40, name: 'Fulano' },
        server_name: 'Meu servidor',
        channel_name: 'Geral',
        duration_ms: 187_000,
        size_bytes: 1000,
        created_at: new Date(now).toISOString(),
        expires_at: new Date(now + 7 * DAY).toISOString(),
        thumbnail_url: null,
        playlist_url: null,
        download_url: null,
        ...extra,
    });
    const lastInvoke = () => invokes.filter(item => item.command !== 'log_line').at(-1);
    const byId = (id: string) => hub.clips.store.state.clips.find(item => item.id === id)!;

    beforeAll(() => {
        window.__TAURI__ = {
            core: {
                invoke: async (command, args) => {
                    invokes.push({ command, args });

                    return null;
                },
            },
            event: { listen: async () => () => null },
        };
        Object.assign(window.HTMLMediaElement.prototype, { play: async () => undefined, pause() {}, load() {} });

        Object.defineProperty(navigator, 'platform', { value: 'Win32', configurable: true });

        app = new App();
        hub = app.hub;
        app.toast = message => toasts.push(message);
        hub.api.request = async (method: string, path: string, body: unknown) => {
            calls.push({ method, path, body });

            const answer = responses.get(`${method} ${path}`);

            if (answer instanceof Error) {
                throw answer;
            }

            return typeof answer === 'function' ? answer(body) : answer ?? null;
        };
    });

    afterAll(() => {
        hub.echo?.disconnect();
    });

    it('um clipe recém-feito some em 7 dias, e não 6 por arredondar para baixo', () => {
        expect(Clips.duration(187_000)).toBe('3:07');
        expect(Clips.expiry(new Date(now + 7 * DAY - 60_000).toISOString(), now)).toBe('some em 7 dias');
        expect(Clips.expiry(new Date(now + 5 * 3_600_000).toISOString(), now)).toBe('some em 5 h');
        expect(Clips.expiry(new Date(now + 60_000).toISOString(), now)).toBe('some em menos de 1 h');
    });

    it('sem conta, a aba só troca e nada é buscado: a entrada segue onde estava', () => {
        app.showEntry();
        app.setEntry({ entryCode: 'minha-sala' });
        app.setTab('clips');

        expect(app.store.state.tab).toBe('clips');
        expect(calls.length, 'sem conta, nenhuma busca').toBe(0);

        app.setTab('broadcast');
        expect(app.store.state.tab).toBe('broadcast');
        expect(app.store.state.screen).toBe('entry');
        expect(app.store.state.entryCode).toBe('minha-sala');
    });

    it('entrar pela aba Clipes é o login de sempre, e a lista aparece', async () => {
        responses.set('POST /api/auth/login', { token: 'sanctum', user: { id: 1, name: 'Edsu' } });
        responses.set('GET /api/config', { reverb: { host: '127.0.0.1', port: 9, key: 'key', scheme: 'http' } });
        responses.set('GET /api/servers', []);
        responses.set('GET /api/clips', [
            clip('ready', 'ready', {
                thumbnail_url: 'http://minio/thumb.jpg',
                playlist_url: 'http://api/clips/ready/playlist.m3u8?signature=a',
                download_url: 'http://minio/clips/ready/clip.mp4?X-Amz-Signature=c',
            }),
            clip('processing', 'processing', { duration_ms: null }),
            clip('failed', 'failed'),
        ]);

        app.setTab('clips');
        expect(await hub.login('edsu@example.com', 'secret')).toBe(true);
        await new Promise(resolve => setTimeout(resolve, 0));

        expect(app.store.state.screen).toBe('hub');
        expect(calls.some(call => call.method === 'GET' && call.path === '/api/clips'), 'ao logar com a aba aberta, a lista vem').toBe(true);
        expect(hub.clips.store.state.clips.map(item => item.status)).toEqual(['ready', 'processing', 'failed']);
        expect(hub.clips.store.state.loading).toBe(false);
    });

    it('senha errada vira texto no cartão, e não exceção', async () => {
        responses.set('POST /api/auth/login', Object.assign(new Error('Credenciais inválidas.'), { status: 422 }));

        expect(await hub.login('edsu@example.com', 'errada')).toBe(false);
        expect(hub.store.state.loginError).toBe('Credenciais inválidas.');
        expect(hub.store.state.loginBusy).toBe(false);
    });

    it('Google sem resposta do navegador: o botão volta a funcionar e o aviso pede para tentar de novo', async () => {
        const bridge = window.__TAURI__!.core.invoke;

        window.__TAURI__!.core.invoke = async (command, args) => {
            if (command === 'google_login') {
                throw new Error('porta fechada');
            }

            return bridge(command, args);
        };

        expect(await hub.googleLogin()).toBe(false);
        expect(hub.store.state.googleWaiting, 'o botão do Google não fica preso esperando').toBe(false);
        expect(hub.store.state.loginError).toMatch(/Tente de novo/);

        window.__TAURI__!.core.invoke = bridge;
    });

    it('assistir usa o HLS nativo com a URL assinada intacta, e fechar solta o vídeo', () => {
        window.HTMLMediaElement.prototype.canPlayType = type => (type === 'application/vnd.apple.mpegurl' ? 'maybe' : '');

        const video = document.createElement('video');

        hub.clips.play(byId('ready'));
        expect(hub.clips.store.state.playing?.id).toBe('ready');

        hub.clips.attach(video);
        expect(video.getAttribute('src')).toBe('http://api/clips/ready/playlist.m3u8?signature=a');

        hub.clips.closePlayer();
        hub.clips.detach(video);
        expect(hub.clips.store.state.playing).toBeNull();
        expect(video.hasAttribute('src'), 'fechar solta o vídeo').toBe(false);
    });

    it('baixar vai pelo navegador do sistema, com a URL assinada como veio', async () => {
        await hub.clips.download(byId('ready'));

        expect(lastInvoke()).toEqual({ command: 'open_url', args: { url: 'http://minio/clips/ready/clip.mp4?X-Amz-Signature=c' } });
    });

    it('o ClipUpdated pelo canal da conta troca o cartão em processamento, sem duplicar', () => {
        hub.echo!.private('user.1').subscription.emit('ClipUpdated', {
            clip: clip('processing', 'ready', { playlist_url: 'http://api/clips/processing/playlist.m3u8?signature=b' }),
        });

        expect(byId('processing').status).toBe('ready');
        expect(hub.clips.store.state.clips.length, 'atualizar não duplica').toBe(3);
    });

    it('apagar pergunta antes; recusou, nada sai', async () => {
        app.confirm = async () => false;
        await hub.clips.remove(byId('failed'));
        expect(calls.some(call => call.method === 'DELETE'), 'sem confirmar, sem DELETE').toBe(false);

        app.confirm = async () => true;
        await hub.clips.remove(byId('failed'));
        expect(calls.at(-1)).toEqual({ method: 'DELETE', path: '/api/clips/failed', body: undefined });
        expect(hub.clips.store.state.clips.find(item => item.id === 'failed')).toBeUndefined();
        expect(hub.clips.store.state.clips.length).toBe(2);
    });

    it('o Clipar só aparece com alguém transmitindo e manda o user_id que o SFU diz estar transmitindo', async () => {
        const voice = hub.voice;

        hub.tree = {
            id: 'server',
            name: 'Meu servidor',
            channels: [{ id: 'voice', type: 'voice', name: 'Voz', position: 0, permissions: 0 }],
            voice: { voice: [{ user_id: 1, name: 'Edsu', sources: [] }, { user_id: 40, name: 'Fulano', sources: [] }] },
            members: [],
            roles: [],
            me: { permissions: 0, top_position: 0 },
        };
        voice.channel = hub.tree.channels[0];
        app.media.sfu = {
            peers: new Map([
                ['me', { peerId: 'me', name: 'Edsu', self: true, sharing: false, producers: [] }],
                ['fulano', { peerId: 'fulano', userId: 'user:40', name: 'Fulano', sharing: false, producers: [] }],
            ]),
        };

        expect(voice.streamers(), 'ninguém transmitindo, nada de Clipar').toEqual([]);

        Object.assign(app.media.sfu.peers.get('fulano'), { sharing: true, producers: [{ producerId: 'screen', kind: 'video', source: 'screen' }] });
        expect(voice.streamers().map(streamer => streamer.name), 'o Fulano compartilha: Clipar aparece').toEqual(['Fulano']);

        voice.toggleClipList();
        expect(voice.store.state.clipOpen).toBe(true);

        responses.set('POST /api/channels/voice/clips', (body: { user_id: number }) => clip('fresh', 'processing', { streamer: { id: body.user_id, name: 'Fulano' } }));
        await voice.clip(voice.streamers()[0]);

        expect(calls.at(-1), 'o user_id vem do user:<id> do SFU').toEqual({ method: 'POST', path: '/api/channels/voice/clips', body: { user_id: 40 } });
        expect(toasts.at(-1)).toMatch(/Clipando os últimos 5 min/);
        expect(voice.store.state.clipOpen).toBe(false);
        expect(hub.clips.store.state.clips[0].id, 'o 202 já aparece na aba, em processamento').toBe('fresh');
    });

    it('eu também transmitindo entro na lista com o meu id, e não com o do peer', () => {
        app.sharing.store.set({ active: true });

        expect(hub.voice.streamers().map(streamer => streamer.userId)).toEqual([1, 40]);
    });

    it('a recusa da API (a pessoa parou antes do clique) vira aviso com a mensagem dela', async () => {
        responses.set('POST /api/channels/voice/clips', Object.assign(new Error('Essa pessoa não está transmitindo.'), { status: 422 }));

        await hub.voice.clip({ userId: 40, name: 'Fulano' });

        expect(toasts.at(-1)).toBe('Essa pessoa não está transmitindo.');
    });

    it('a lista aberta do Clipar fecha quando a última pessoa para de transmitir, e não reabre sozinha depois', () => {
        const fulano = app.media.sfu!.peers.get('fulano')!;

        hub.voice.toggleClipList();
        expect(hub.voice.store.state.clipOpen).toBe(true);

        Object.assign(fulano, { sharing: false, producers: [] });
        hub.syncVoiceSources();
        expect(hub.voice.store.state.clipOpen, 'ainda sou eu transmitindo: a lista segue aberta').toBe(true);

        app.sharing.paint(false);
        expect(hub.voice.store.state.clipOpen, 'parei eu também: a lista fecha').toBe(false);

        Object.assign(fulano, { sharing: true, producers: [{ producerId: 'screen', kind: 'video', source: 'screen' }] });
        hub.voice.toggleClipList();
        expect(hub.voice.store.state.clipOpen).toBe(true);

        Object.assign(fulano, { sharing: false, producers: [] });
        hub.syncVoiceSources();
        expect(hub.voice.store.state.clipOpen, 'o Fulano era o último e parou: a lista fecha').toBe(false);
    });

    it('sair da conta não deixa lista nem player para quem entrar depois', () => {
        hub.clips.forget();

        expect(hub.clips.store.state.clips.length).toBe(0);
    });

    it('fora do Windows não existe Clipes: a aba não troca e nada é buscado', () => {
        Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true });

        const before = calls.length;

        app.setTab('broadcast');
        app.setTab('clips');

        expect(app.store.state.tab, 'a aba segue na transmissão').toBe('broadcast');
        expect(calls.length, 'nenhuma busca de clipes').toBe(before);

        Object.defineProperty(navigator, 'platform', { value: 'Win32', configurable: true });
    });
});
