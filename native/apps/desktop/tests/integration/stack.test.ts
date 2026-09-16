import Echo from 'laravel-echo';
import Pusher from 'pusher-js';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { ApiClient } from '../../ui/core/ApiClient.ts';
import { SfuClient } from '../../ui/core/SfuClient.ts';

const SERVER = process.env.UNKVOID_SERVER ?? 'http://127.0.0.1:8000';
const STAMP = Date.now();
const PASSWORD = 'senha-de-integracao-123';
const memory = new Map<string, string>();

vi.stubGlobal('localStorage', {
    getItem: (key: string) => memory.get(key) ?? null,
    setItem: (key: string, value: unknown) => memory.set(key, String(value)),
    removeItem: (key: string) => memory.delete(key),
});

const waitFor = async <Value>(label: string, predicate: () => Value | Promise<Value>, timeoutMs = 10_000): Promise<Value> => {
    const started = Date.now();

    while (Date.now() - started < timeoutMs) {
        const value = await predicate();

        if (value) {
            return value;
        }

        await new Promise(resolve => setTimeout(resolve, 100));
    }

    throw new Error(`esperou ${timeoutMs} ms por: ${label}`);
};

const listen = (subscription, name: string, handler: (payload) => void) => subscription.listen(`.${name}`, handler).listen(name, handler);

describe('integração: os clientes do app contra o Laravel, o Reverb e o SFU no ar', () => {
    const cleanup: (() => unknown)[] = [];
    const ana = new ApiClient(SERVER);
    const bia = new ApiClient(SERVER);
    const anaSfu = new SfuClient();
    const biaSfu = new SfuClient();
    let config;
    let anaEcho;
    let biaEcho;
    let biaUser;
    let created;
    let text;
    let voice;
    let tree;

    const echoFor = (client: ApiClient) => new Echo({
        broadcaster: 'reverb',
        Pusher,
        key: config.reverb.key,
        wsHost: config.reverb.host,
        wsPort: config.reverb.port,
        wssPort: config.reverb.port,
        forceTLS: config.reverb.scheme === 'https',
        enabledTransports: ['ws', 'wss'],
        authEndpoint: `${SERVER}/broadcasting/auth`,
        auth: { headers: { Authorization: `Bearer ${client.token}` } },
    });
    const subscribed = (echo, channel: string) => waitFor(`inscrição em ${channel}`, () => echo.connector.pusher.channel(channel)?.subscribed);
    const rejectsWith = async (work: () => Promise<unknown>, statuses: number[], message: string) => {
        const failure = await work().then(() => null, (reason: { status?: number }) => reason);

        expect(statuses, message).toContain(failure?.status);
    };

    beforeAll(async () => {
        try {
            config = await new ApiClient(SERVER).get('/api/config');
        } catch (failure) {
            throw new Error(`o Laravel não respondeu em ${SERVER} (${(failure as Error).message}). Suba a pilha local antes.`);
        }
    });

    afterAll(async () => {
        for (const step of cleanup.reverse()) {
            await Promise.resolve().then(step).catch((failure: Error) => console.error('limpeza:', failure.message));
        }
    });

    it('cria duas contas, o token abre o /api/me, e senha errada é 401', async () => {
        ana.setToken((await ana.post('/api/auth/register', { name: `ana.${STAMP}`, email: `ana.${STAMP}@local.test`, password: PASSWORD, device: 'app' })).token);
        bia.setToken((await bia.post('/api/auth/register', { name: `bia.${STAMP}`, email: `bia.${STAMP}@local.test`, password: PASSWORD, device: 'app' })).token);

        const anaUser = await ana.get('/api/me');

        biaUser = await bia.get('/api/me');

        expect(anaUser.name).toBe(`ana.${STAMP}`);
        await rejectsWith(() => new ApiClient(SERVER).post('/api/auth/login', { email: `ana.${STAMP}@local.test`, password: 'errada', device: 'app' }), [401], 'senha errada é 401 (InvalidCredentialsException)');
    });

    it('o servidor nasce com um canal de texto e um de voz, e o convite põe a outra conta dentro', async () => {
        created = await ana.post('/api/servers', { name: `Integração ${STAMP}` });
        cleanup.push(() => ana.delete(`/api/servers/${created.id}`));

        tree = await ana.get(`/api/servers/${created.id}`);
        text = tree.channels.find(channel => channel.type === 'text');
        voice = tree.channels.find(channel => channel.type === 'voice');

        expect(text && voice, 'o servidor nasce com um canal de texto e um de voz').toBeTruthy();
        expect(tree.invite_code, 'o dono vê o convite').toBeTruthy();

        await bia.post(`/api/invites/${tree.invite_code}`);
        expect((await bia.get('/api/servers')).some(server => server.id === created.id), 'a convidada vê o servidor na lista').toBe(true);
    });

    it('quem autoriza é o Laravel: membro comum não renomeia', async () => {
        await rejectsWith(() => bia.patch(`/api/servers/${created.id}`, { name: 'tomado' }), [403], 'membro comum recebe 403');
    });

    it('enviar, editar e apagar mensagem chegam na outra conta pelo Reverb', async () => {
        const sent = [];
        const updated = [];
        const deleted = [];

        anaEcho = echoFor(ana);
        biaEcho = echoFor(bia);
        cleanup.push(() => anaEcho.disconnect(), () => biaEcho.disconnect());

        const textSubscription = biaEcho.private(`channel.${text.id}`);

        listen(textSubscription, 'MessageSent', ({ message }) => sent.push(message));
        listen(textSubscription, 'MessageUpdated', ({ message }) => updated.push(message));
        listen(textSubscription, 'MessageDeleted', payload => deleted.push(payload));
        await subscribed(biaEcho, `private-channel.${text.id}`);

        const message = await ana.post(`/api/channels/${text.id}/messages`, { body: 'oi da integração' });

        await waitFor('MessageSent na outra conta', () => sent.find(item => item.id === message.id));
        await ana.patch(`/api/messages/${message.id}`, { body: 'editada' });
        await waitFor('MessageUpdated na outra conta', () => updated.find(item => item.id === message.id && item.body === 'editada'));
        await rejectsWith(() => bia.patch(`/api/messages/${message.id}`, { body: 'não é minha' }), [403], 'só o autor edita');
        await ana.delete(`/api/messages/${message.id}`);
        await waitFor('MessageDeleted na outra conta', () => deleted.find(item => item.id === message.id));

        expect((await bia.get(`/api/channels/${text.id}/messages`)).some(item => item.id === message.id), 'apagada some do histórico').toBe(false);
    });

    it('a voz pede um token por join, e a outra conta vê chegar pelo SFU e pelo webhook', async () => {
        const voiceStates = [];
        const peerJoined = [];
        const voiceSubscription = anaEcho.private(`channel.${voice.id}`);
        const tokenFor = (client: ApiClient) => async () => ({ token: (await client.post(`/api/channels/${voice.id}/voice/token`)).token });

        listen(voiceSubscription, 'VoiceStateUpdated', payload => voiceStates.push(payload));
        await subscribed(anaEcho, `private-channel.${voice.id}`);

        cleanup.push(() => anaSfu.disconnect(), () => biaSfu.disconnect());
        anaSfu.addEventListener('peerJoined', event => peerJoined.push((event as CustomEvent).detail));

        const anaJoined = await anaSfu.connect(config.sfu, tokenFor(ana));

        expect([...anaJoined.can].sort(), 'o dono entra com speak, stream e video').toEqual(['speak', 'stream', 'video']);

        const biaJoined = await biaSfu.connect(config.sfu, tokenFor(bia));

        expect(biaJoined.peers.some(peer => peer.name === `ana.${STAMP}`), 'quem chega vê quem já estava').toBe(true);
        await waitFor('peerJoined para quem já estava', () => peerJoined.find(peer => peer.name === `bia.${STAMP}`));
        await waitFor('VoiceStateUpdated (SFU → webhook → Laravel → Reverb)', () => voiceStates.find(item => item.event === 'joined' && item.user_id === biaUser.id));
        await waitFor('a árvore mostra quem está na voz', async () => (await ana.get(`/api/servers/${created.id}`)).voice?.[voice.id]?.some(person => person.user_id === biaUser.id), 15_000);
    });

    it('a tela sobe pelo producePlain com a oferta do Rust, e a outra conta vê a pessoa transmitindo', async () => {
        const newProducers = [];

        biaSfu.addEventListener('newProducer', event => newProducers.push((event as CustomEvent).detail));

        const screen = await anaSfu.request('producePlain', {
            kind: 'video',
            source: 'screen',
            rtpParameters: {
                codecs: [{ mimeType: 'video/H264', payloadType: 96, clockRate: 90_000, parameters: { 'packetization-mode': 1, 'level-asymmetry-allowed': 1, 'profile-level-id': '42e01f' }, rtcpFeedback: [{ type: 'nack' }, { type: 'nack', parameter: 'pli' }] }],
                encodings: [{ ssrc: 0x2234_5678 }],
            },
            srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: Buffer.from(crypto.getRandomValues(new Uint8Array(30))).toString('base64') },
        });

        await waitFor('newProducer da tela na outra conta', () => newProducers.find(item => item.producerId === screen.producerId));
        expect(biaSfu.peers.get(anaSfu.peerId!)?.sharing, 'quem assiste vê a pessoa transmitindo').toBe(true);
    });

    it('mutar pelo servidor chega na pessoa pelo SFU', async () => {
        const serverMuted = [];

        biaSfu.addEventListener('serverMuted', event => serverMuted.push((event as CustomEvent).detail));
        await ana.patch(`/api/servers/${created.id}/members/${biaUser.id}`, { server_mute: true });

        await waitFor('serverMuted na pessoa mutada', () => serverMuted.find(item => item.muted === true));
    });

    it('expulsar manda MemberRemoved no canal da conta e kicked no SFU, e o servidor some para ela', async () => {
        const removed = [];
        const kicked = [];
        const ownSubscription = biaEcho.private(`user.${biaUser.id}`);

        listen(ownSubscription, 'MemberRemoved', payload => removed.push(payload));
        await subscribed(biaEcho, `private-user.${biaUser.id}`);
        biaSfu.addEventListener('kicked', event => kicked.push((event as CustomEvent).detail ?? {}));

        await ana.delete(`/api/servers/${created.id}/members/${biaUser.id}`);

        await waitFor('MemberRemoved no canal da conta', () => removed.find(item => item.server_id === created.id && item.reason === 'kicked'));
        await waitFor('kicked no SFU', () => kicked.length > 0);
        await rejectsWith(() => bia.get(`/api/servers/${created.id}`), [403, 404], 'expulsa não abre mais o servidor');
        await rejectsWith(() => bia.post(`/api/channels/${voice.id}/voice/token`), [403, 404], 'e não ganha token de voz');
    });

    it('na sala por código, sem conta e sem token, dois entram e se veem; o canal de voz sem token é recusado', async () => {
        const room = `integracao-${STAMP}`;
        const first = new SfuClient();
        const second = new SfuClient();
        const intruder = new SfuClient();

        cleanup.push(() => first.disconnect(), () => second.disconnect(), () => intruder.disconnect());
        await first.connect(config.sfu, { room, name: 'Primeira', installId: `install-1-${STAMP}` });

        const secondJoined = await second.connect(config.sfu, { room, name: 'Segunda', installId: `install-2-${STAMP}` });

        expect(secondJoined.peers.some(peer => peer.name === 'Primeira'), 'a sala por código junta quem tem o código').toBe(true);
        await expect(intruder.connect(config.sfu, { room: voice.id, name: 'Intrusa', installId: `install-3-${STAMP}` }), 'canal de voz sem token é recusado').rejects.toThrow();
    });

    it('a lista de clipes de uma conta nova responde vazia', async () => {
        expect(await ana.get('/api/clips')).toEqual([]);
    });
});
