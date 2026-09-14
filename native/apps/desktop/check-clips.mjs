/**
 * A aba Clipes e o Clipar, com o DOM de verdade e a API de mentira.
 *
 * Trocar de aba não pode encostar na Transmissão: é lá que mora a sala por código, que
 * não sabe nada de conta. E o Clipar manda o `user_id` que o SFU diz estar transmitindo
 * — errar o elo `user:<id>` é clipar a pessoa errada, ou ninguém.
 *
 * node check-clips.mjs
 */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

import { JSDOM } from 'jsdom';

const dom = new JSDOM(await readFile('./ui/index.html', 'utf8'), { url: 'http://localhost/' });
const { window } = dom;
const invokes = [];

window.__TAURI__ = {
    core: {
        invoke: async (command, args) => {
            invokes.push({ command, args });

            return null;
        },
    },
    event: { listen: async () => null },
};
Object.assign(window.HTMLMediaElement.prototype, { play: async () => null, pause() {}, load() {} });

globalThis.window = window;
globalThis.document = window.document;
Object.defineProperty(window.document, 'hidden', { value: false, configurable: true });
globalThis.localStorage = window.localStorage;
globalThis.sessionStorage = window.sessionStorage;
globalThis.location = window.location;
Object.defineProperty(globalThis, 'navigator', { value: window.navigator, configurable: true });

const { App } = await import('./ui/app.js');
const { Clips } = await import('./ui/Clips.js');

const app = new App();
const hub = app.hub;
const el = id => document.getElementById(id);
const card = id => document.querySelector(`[data-clip="${id}"]`);
const settle = () => new Promise(resolve => setTimeout(resolve, 0));
const toasts = [];
const calls = [];
const responses = new Map();
const now = Date.now();
const DAY = 86_400_000;

const clip = (id, status, extra = {}) => ({
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

app.toast = message => toasts.push(message);
hub.api.request = async (method, path, body) => {
    calls.push({ method, path, body });

    const answer = responses.get(`${method} ${path}`);

    if (answer instanceof Error) {
        throw answer;
    }

    return typeof answer === 'function' ? answer(body) : answer ?? null;
};

// O clipe some em 7 dias: um clipe recém-feito diz 7, e não 6 por arredondar para baixo.
assert.equal(Clips.duration(187_000), '3:07');
assert.equal(Clips.expiry(new Date(now + 7 * DAY - 60_000).toISOString(), now), 'some em 7 dias');
assert.equal(Clips.expiry(new Date(now + 5 * 3_600_000).toISOString(), now), 'some em 5 h');
assert.equal(Clips.expiry(new Date(now + 60_000).toISOString(), now), 'some em menos de 1 h');

// ---- Sem conta: a Transmissão é a sala por código, e a aba Clipes só esconde e devolve.
app.showEntry();
el('room-code').value = 'minha-sala';

const screens = () => ['entry-screen', 'hub', 'room'].map(id => el(id).hidden);
const before = screens();
const loginPlace = [...el('entry-cards').children].indexOf(el('login-panel'));

el('tab-clips').click();
assert.equal(el('broadcast-view').hidden, true);
assert.equal(el('clips-view').hidden, false);
assert.equal(el('tab-clips').getAttribute('aria-selected'), 'true');
assert.ok(el('clips-login-slot').contains(el('login-panel')), 'sem token, a aba mostra o mesmo painel de entrar');
assert.equal(document.body.dataset.account, undefined);
assert.equal(calls.length, 0, 'sem token, nenhuma busca');

el('tab-broadcast').click();
assert.equal(el('broadcast-view').hidden, false);
assert.equal(el('clips-view').hidden, true);
assert.deepEqual(screens(), before, 'entrada, servidor e sala voltam como estavam');
assert.equal([...el('entry-cards').children].indexOf(el('login-panel')), loginPlace, 'o painel de entrar volta para o mesmo lugar');
assert.equal(el('room-code').value, 'minha-sala');

// ---- Entrar pela aba Clipes: o login de sempre, e a lista aparece.
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

el('tab-clips').click();
el('login-email').value = 'edsu@example.com';
el('login-password').value = 'secret';
await el('login-form').onsubmit(new window.Event('submit'));
await settle();

assert.equal(document.body.dataset.account, 'on');
assert.ok(calls.some(call => call.method === 'GET' && call.path === '/api/clips'), 'ao logar, a lista vem');
assert.equal(card('ready').querySelector('[data-clip-play]').hidden, false);
assert.equal(card('ready').querySelector('[data-clip-image]').getAttribute('src'), 'http://minio/thumb.jpg');
assert.match(card('ready').querySelector('[data-clip-when]').textContent, /3:07 · some em 7 dias$/);
assert.equal(card('processing').querySelector('[data-clip-processing]').hidden, false);
assert.equal(card('processing').querySelector('[data-clip-play]').hidden, true, 'sem playlist, sem Assistir');
assert.equal(card('failed').querySelector('[data-clip-failed]').hidden, false);
assert.equal(card('failed').querySelector('[data-clip-play]').hidden, true);
assert.equal(card('ready').querySelector('[data-clip-failed]').hidden, true);

// ---- Assistir: no jsdom não há Media Source, então é o HLS nativo — com a URL assinada
// intacta, sem cabeçalho nenhum.
window.HTMLMediaElement.prototype.canPlayType = type => type === 'application/vnd.apple.mpegurl' ? 'maybe' : '';
card('ready').querySelector('[data-clip-play]').click();
assert.equal(el('clip-player').hidden, false);
assert.equal(el('clip-video').getAttribute('src'), 'http://api/clips/ready/playlist.m3u8?signature=a');
el('clip-player-close').click();
assert.equal(el('clip-player').hidden, true);
assert.equal(el('clip-video').hasAttribute('src'), false, 'fechar solta o vídeo');

// ---- Baixar vai pelo navegador do sistema, com a URL assinada como veio.
assert.equal(card('processing').querySelector('[data-clip-download]').hidden, true, 'sem download_url, sem Baixar');
card('ready').querySelector('[data-clip-download]').click();
await settle();
assert.deepEqual(invokes.at(-1), { command: 'open_url', args: { url: 'http://minio/clips/ready/clip.mp4?X-Amz-Signature=c' } });

// ---- `ClipUpdated` pelo canal da conta: o cartão em processamento vira pronto.
hub.echo.private('user.1').subscription.emit('ClipUpdated', {
    clip: clip('processing', 'ready', { playlist_url: 'http://api/clips/processing/playlist.m3u8?signature=b' }),
});
assert.equal(card('processing').dataset.status, 'ready');
assert.equal(card('processing').querySelector('[data-clip-processing]').hidden, true);
assert.equal(card('processing').querySelector('[data-clip-play]').hidden, false);
assert.equal(el('clip-list').children.length, 3, 'atualizar não duplica');

// ---- Apagar pede confirmação; recusou, nada sai.
globalThis.confirm = () => false;
card('failed').querySelector('[data-clip-delete]').click();
await settle();
assert.ok(! calls.some(call => call.method === 'DELETE'), 'sem confirmar, sem DELETE');

globalThis.confirm = () => true;
card('failed').querySelector('[data-clip-delete]').click();
await settle();
assert.deepEqual(calls.at(-1), { method: 'DELETE', path: '/api/clips/failed', body: undefined });
assert.equal(card('failed'), null);
assert.equal(el('clip-list').children.length, 2);

// ---- Clipar: só com alguém transmitindo no canal de voz atual.
hub.tree = {
    id: 'server',
    name: 'Meu servidor',
    channels: [{ id: 'voice', type: 'voice', name: 'Voz', position: 0, permissions: 0 }],
    voice: { voice: [{ user_id: 1, name: 'Edsu', sources: [] }, { user_id: 40, name: 'Fulano', sources: [] }] },
    members: [],
    roles: [],
    me: { permissions: 0, top_position: 0 },
};
hub.voice.channel = hub.tree.channels[0];
app.sfu = {
    peers: new Map([
        ['me', { peerId: 'me', name: 'Edsu', self: true, sharing: false, producers: [] }],
        ['fulano', { peerId: 'fulano', userId: 'user:40', name: 'Fulano', sharing: false, producers: [] }],
    ]),
};
hub.syncVoiceSources();
assert.equal(el('voice-clip').hidden, true, 'ninguém transmitindo, nada de Clipar');

Object.assign(app.sfu.peers.get('fulano'), { sharing: true, producers: [{ producerId: 'screen', kind: 'video', source: 'screen' }] });
hub.syncVoiceSources();
assert.equal(el('voice-clip').hidden, false, 'o Fulano compartilha: Clipar aparece');

el('voice-clip').click();
assert.equal(el('voice-clip-list').hidden, false);
assert.deepEqual([...el('voice-clip-streamers').children].map(button => button.textContent), ['Fulano']);

responses.set('POST /api/channels/voice/clips', body => clip('fresh', 'processing', { streamer: { id: body.user_id, name: 'Fulano' } }));
el('voice-clip-streamers').children[0].click();
await settle();
assert.deepEqual(calls.at(-1), { method: 'POST', path: '/api/channels/voice/clips', body: { user_id: 40 } }, 'o user_id vem do user:<id> do SFU');
assert.match(toasts.at(-1), /Clipando os últimos 5 min/);
assert.equal(el('voice-clip-list').hidden, true);
assert.ok(card('fresh'), 'o 202 já aparece na aba, em processamento');

// Eu também transmito: entro na lista com o meu id, e não com o do peer.
app.sharing = true;
hub.voice.paintBar();
assert.deepEqual(hub.voice.streamers().map(streamer => streamer.userId), [1, 40]);

// A recusa (a pessoa parou de transmitir antes do clique) vira aviso com a mensagem da API.
responses.set('POST /api/channels/voice/clips', Object.assign(new Error('Essa pessoa não está transmitindo.'), { status: 422 }));
await hub.voice.clip({ userId: 40, name: 'Fulano' });
assert.equal(toasts.at(-1), 'Essa pessoa não está transmitindo.');

// Parou todo mundo: o botão some, e a lista aberta fecha junto.
app.sharing = false;
el('voice-clip-list').hidden = false;
Object.assign(app.sfu.peers.get('fulano'), { sharing: false, producers: [] });
hub.syncVoiceSources();
assert.equal(el('voice-clip').hidden, true);
assert.equal(el('voice-clip-list').hidden, true);

// Sair da conta não deixa lista nem player para quem entrar depois.
hub.clips.forget();
assert.equal(el('clip-list').children.length, 0);

hub.echo.disconnect();

console.log('clipes: ok — abas, login, três status, player, baixar, ClipUpdated, apagar e Clipar');
