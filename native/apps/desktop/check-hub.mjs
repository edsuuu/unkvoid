/**
 * O modo servidor, conferido sem abrir o app.
 *
 * Os bits de permissão e a hierarquia de cargos decidem qual botão aparece; um erro
 * aqui não dá exceção, dá botão faltando — ou botão sobrando, que o servidor recusa
 * com 403 na cara de quem clicou. E o token da voz vale 60 s: se o `join` de uma
 * reconexão reaproveitar o velho, o SFU recusa e a pessoa cai sem entender.
 *
 * node check-hub.mjs
 */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

import { JSDOM } from 'jsdom';

const { Permissions } = await import('./ui/Permissions.js');
const { SfuClient } = await import('./ui/SfuClient.js');

// Os bits batem com a tabela de SERVIDORES.md. O @everyone padrão é a soma de sete
// deles (31552 — o "32448" do exemplo de árvore do documento não fecha com a tabela).
assert.equal(Permissions.MOVE_MEMBERS, 131072);
assert.equal(Permissions.ALL, 262143);

const EVERYONE = Permissions.VIEW_CHANNEL | Permissions.SEND_MESSAGES | Permissions.CONNECT
    | Permissions.SPEAK | Permissions.STREAM | Permissions.VIDEO | Permissions.CREATE_INVITE;

assert.equal(EVERYONE, 31552);
assert.ok(Permissions.has(EVERYONE, Permissions.SPEAK));
assert.ok(! Permissions.has(EVERYONE, Permissions.KICK_MEMBERS));

// Administrador tem tudo, mesmo com o bit específico apagado.
assert.ok(Permissions.has(Permissions.ADMINISTRATOR, Permissions.BAN_MEMBERS));

const server = {
    owner_id: 1,
    roles: [
        { id: 10, name: '@everyone', position: 0, permissions: EVERYONE, is_everyone: true },
        { id: 11, name: 'mod', position: 2, permissions: Permissions.KICK_MEMBERS | Permissions.MANAGE_MESSAGES, is_everyone: false },
        { id: 12, name: 'vip', position: 1, permissions: 0, is_everyone: false },
    ],
    members: [
        { user_id: 1, role_ids: [10], is_owner: true },
        { user_id: 2, role_ids: [10, 11], is_owner: false },
        { user_id: 3, role_ids: [10, 12], is_owner: false },
        { user_id: 4, role_ids: [10], is_owner: false },
    ],
};

// Hierarquia: só se mexe em quem está abaixo; o dono está acima de todos, inclusive de si.
const [owner, mod, vip, plain] = server.members;

assert.equal(Permissions.topPosition(server, mod), 2);
assert.equal(Permissions.topPosition(server, plain), 0);
assert.ok(Permissions.outranks(server, mod, vip));
assert.ok(Permissions.outranks(server, owner, mod));
assert.ok(! Permissions.outranks(server, vip, mod));
assert.ok(! Permissions.outranks(server, mod, mod), 'ninguém está acima de si mesmo');
assert.ok(! Permissions.outranks(server, mod, owner));

// O token da voz é pedido antes de CADA join: a função é chamada de novo na reconexão.
const client = new SfuClient();
const tokens = [];
const joins = [];

client.identity = async () => {
    tokens.push(`token-${tokens.length + 1}`);

    return { token: tokens.at(-1) };
};
client.request = async (action, data) => {
    if (action === 'join') {
        joins.push(data.token);
    }

    return { peerId: 'me', resumeKey: 'k', resumed: true, peers: [] };
};

await client.setup();
await client.setup();
assert.deepEqual(joins, ['token-1', 'token-2'], 'cada join leva um token novo');

// A sala anônima continua mandando o objeto puro.
client.identity = { room: 'sala', name: 'Edsu', installId: 'i' };
await client.setup();
assert.equal(joins.length, 3);

// Publicar: o transporte de envio nasce na primeira publicação, e o `produce` do
// mediasoup só vira producer quando o servidor devolve o id pelo `on('produce')`.
// Pausar fala com o servidor E com o producer local — só o local continuaria
// subindo silêncio.
const actions = [];
const handlers = new Map();
const transport = {
    id: 'send-1',
    closed: false,
    on: (event, handler) => handlers.set(event, handler),
    close() { this.closed = true; },
    produce: ({ track, appData, ...options }) => new Promise((resolve, reject) => {
        handlers.get('produce')(
            { kind: 'audio', rtpParameters: { codecs: [] }, appData },
            ({ id }) => resolve({ id, track, appData, options, paused: false, pause() { this.paused = true; }, resume() { this.paused = false; }, close() {}, on() {} }),
            reject,
        );
    }),
};

client.device = { rtpCapabilities: {}, createSendTransport: () => transport };
client.request = async (action, data = {}) => {
    actions.push(`${action}:${data.producerId ?? data.consumerId ?? data.source ?? ''}`);

    return action === 'createTransport' ? { transportId: 'send-1' } : action === 'produce' ? { producerId: 'p1' } : {};
};

const [published, twin] = await Promise.all([
    client.produce({}, 'mic', { codecOptions: { opusDtx: true } }),
    client.produce({}, 'camera'),
]);

assert.equal(published.id, 'p1', 'o id do producer é o que o servidor devolveu no on(produce)');
assert.equal(published.appData.source, 'mic');
assert.deepEqual(published.options, { codecOptions: { opusDtx: true } }, 'as opções chegam ao produce do mediasoup');
assert.equal(actions.filter(action => action === 'createTransport:').length, 1, 'mic e câmera juntos criam UM transporte');
assert.deepEqual(actions.filter(action => action.startsWith('produce:')), ['produce:mic', 'produce:camera']);
assert.ok(client.sendTransport, 'o transporte de envio existe depois do primeiro produce');
assert.equal(twin.appData.source, 'camera');

actions.length = 0;
await client.pauseProducer('p1');
assert.ok(client.producers.get('p1').paused);
assert.deepEqual(actions, ['pauseProducer:p1']);
await client.resumeProducer('p1');
assert.ok(! client.producers.get('p1').paused);
await client.closeProducer('p1');
assert.ok(! client.producers.has('p1'));
assert.deepEqual(actions, ['pauseProducer:p1', 'resumeProducer:p1', 'closeProducer:p1']);

// Sessão nova (`resumed: false`): o transporte de envio morreu no servidor e não pode
// sobrar aqui — o próximo produce precisa criar outro, e não publicar num fantasma.
client.recvTransport = { close() {} };
client.request = async action => action === 'join'
    ? { peerId: 'me2', resumeKey: 'k2', resumed: false, name: 'Edsu', peers: [] }
    : {};
await client.setup();
assert.equal(client.sendTransport, null, 'sem transporte de envio depois de uma sessão nova');
assert.equal(client.sendTransportPromise, null);
assert.ok(transport.closed, 'o transporte antigo foi fechado');

// ---- Com o DOM de verdade: a casa do modo servidor e a voz que entra mutada.
const dom = new JSDOM(await readFile('./ui/index.html', 'utf8'), { url: 'http://localhost/' });
const { window } = dom;
const invokes = [];

window.__TAURI__ = {
    core: {
        invoke: async (command, args) => {
            invokes.push({ command, args });

            return command === 'sfu_offer' ? { ssrc: 7, payloadType: 111 } : null;
        },
    },
    event: { listen: async () => null },
};
globalThis.window = window;
globalThis.document = window.document;
Object.defineProperty(window.document, 'hidden', { value: false, configurable: true });
globalThis.localStorage = window.localStorage;
globalThis.sessionStorage = window.sessionStorage;
globalThis.location = window.location;
Object.defineProperty(globalThis, 'navigator', { value: window.navigator, configurable: true });

const { App } = await import('./ui/app.js');
const app = new App();
const hub = app.hub;
const el = id => document.getElementById(id);
const settle = () => new Promise(resolve => setTimeout(resolve, 0));
const calls = [];
const responses = new Map();
const quiet = { here() { return this; }, joining() { return this; }, leaving() { return this; }, listen() { return this; } };

app.toast = () => null;
hub.api.request = async (method, path, body) => {
    calls.push(`${method} ${path}`);

    const answer = responses.get(`${method} ${path}`);

    return typeof answer === 'function' ? answer(body) : answer ?? null;
};
hub.user = { id: 1, name: 'Edsu' };
hub.config = {};
hub.echo = { private: () => quiet, join: () => quiet, leave() {}, disconnect() {} };

const tree = (id, name) => ({
    id,
    name,
    owner_id: 1,
    invite_code: `convite-${id}`,
    me: { user_id: 1, permissions: Permissions.ALL, top_position: 2147483647 },
    roles: [],
    channels: [
        { id: `text-${id}`, name: 'geral', type: 'text', topic: null, position: 0, permissions: EVERYONE },
        { id: `voice-${id}`, name: 'Geral', type: 'voice', topic: null, position: 1, permissions: EVERYONE },
    ],
    members: [{ user_id: 1, name: 'Edsu', nickname: null, role_ids: [], server_mute: false, server_deaf: false, is_owner: true }],
    voice: { [`voice-${id}`]: [{ user_id: 40, name: 'Fulano', sources: ['mic'] }] },
});
let servers = [
    { id: 2, name: 'Jogatina', owner_id: 1, last_accessed_at: '2026-09-12T20:00:00Z' },
    { id: 3, name: 'Trampo', owner_id: 9, last_accessed_at: null },
];

responses.set('GET /api/servers', () => servers);
responses.set('GET /api/servers/2', tree(2, 'Jogatina'));
responses.set('GET /api/servers/5', tree(5, 'Nova'));
responses.set('GET /api/channels/text-2/messages', []);
responses.set('GET /api/channels/text-5/messages', []);
responses.set('POST /api/servers', ({ name }) => {
    servers = [{ id: 5, name, owner_id: 1, last_accessed_at: null }, ...servers];

    return { id: 5, name, owner_id: 1 };
});

// Logado e sem servidor aberto: a casa, e nada abre sozinho.
await hub.open();
assert.equal(hub.tree, null, 'o primeiro servidor não abre sozinho');
assert.equal(el('hub-home').hidden, false);
assert.equal(el('hub-empty').hidden, true);
assert.deepEqual([...el('home-servers').children].map(row => row.querySelector('span').textContent), ['Jogatina', 'Trampo'], 'na ordem em que o Laravel mandou');

// Uma das últimas salas: abre o servidor e mostra quem está na voz, sem entrar nela.
el('home-servers').children[0].click();
await settle();
assert.equal(hub.tree?.id, 2);
assert.equal(el('hub-home').hidden, true);
assert.match(el('channel-list').textContent, /Fulano/, 'quem está em cada voz aparece');
assert.equal(hub.voice.channel, null, 'abrir não é entrar na voz');
assert.ok(! calls.some(call => call.includes('/voice/token')));

// Criar sala: pede o nome, abre a sala nova sem entrar na voz e mostra o convite.
await hub.closeServer();
assert.equal(el('hub-home').hidden, false);
el('home-create-name').value = 'Nova';
el('home-create-form').dispatchEvent(new window.Event('submit', { cancelable: true }));
await settle();
assert.ok(calls.includes('POST /api/servers'));
assert.equal(hub.tree?.id, 5, 'criar abre a sala nova');
assert.equal(hub.voice.channel, null, 'e não entra na voz');
assert.equal(el('invite-banner').hidden, false);
assert.equal(el('invite-banner-code').textContent, 'convite-5');
assert.ok(! calls.some(call => call.includes('/voice/token')));

// ---- Entrar na voz liga o mic mutado, e mutado ANTES de publicar.
const micSteps = [];
const track = { enabled: true, stop() {} };

Object.defineProperty(window.navigator, 'mediaDevices', { value: { getUserMedia: async () => ({ getAudioTracks: () => [track] }) }, configurable: true });
app.tearDownMedia = async () => {
    app.sfu = null;
};
app.enterRoom = async (sfu, identity, alongside) => {
    app.sfu = {
        peers: new Map(),
        producers: new Map(),
        produce: async (produced, source) => {
            micSteps.push(`produce:${source}:${produced.enabled ? 'on' : 'off'}`);

            return { id: `${source}-1` };
        },
        pauseProducer: async producerId => micSteps.push(`pause:${producerId}`),
        closeProducer: async () => null,
        request: async () => ({ producerId: 'mic-native', ip: '127.0.0.1', port: 40000 }),
    };
    await alongside({ can: ['speak'] });
};

hub.voice.native = () => false;
await hub.voice.join(hub.tree.channels[1]);
assert.equal(hub.voice.muted, true);
assert.deepEqual(micSteps, ['produce:mic:off', 'pause:mic-1'], 'a trilha já sobe desligada, e o producer pausa no servidor');
assert.match(el('voice-mute').textContent, /Mudo/);

// Desmutou, saiu e voltou: entra mutado de novo. No Linux o Rust cala antes do `use_sfu`.
hub.voice.muted = false;
await hub.voice.leave();
invokes.length = 0;
hub.voice.native = () => true;
await hub.voice.join(hub.tree.channels[1]);

const order = invokes.map(({ command, args }) => command === 'set_voice_muted' ? `muted:${args.muted}` : command);

assert.equal(hub.voice.muted, true);
assert.ok(order.indexOf('start_voice') < order.indexOf('muted:true'), `ordem: ${order}`);
assert.ok(order.indexOf('muted:true') < order.indexOf('use_sfu'), 'calado antes de o RTP ter para onde ir');
assert.ok(! order.includes('muted:false'), 'ninguém desmuta sozinho');
await hub.voice.leave();

console.log('servidores: ok — bits, hierarquia, token por join, producers, casa, criar sala e voz mutada');
