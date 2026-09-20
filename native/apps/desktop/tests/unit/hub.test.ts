import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { ApiClient } from '../../ui/core/ApiClient.ts';
import { App } from '../../ui/core/App.ts';
import { Chat } from '../../ui/core/Chat.ts';
import type { Hub } from '../../ui/core/Hub.ts';
import { Members } from '../../ui/core/Members.ts';
import type { Member, Role, ServerTree } from '../../ui/core/Models.ts';
import { Permissions } from '../../ui/core/Permissions.ts';
import { RoomCode } from '../../ui/core/RoomCode.ts';
import type { Voice } from '../../ui/core/Voice.ts';

const EVERYONE = Permissions.VIEW_CHANNEL | Permissions.SEND_MESSAGES | Permissions.CONNECT
    | Permissions.SPEAK | Permissions.STREAM | Permissions.VIDEO | Permissions.CREATE_INVITE;

const SERVER = {
    owner_id: 1,
    roles: [
        { id: 10, name: '@everyone', position: 0, permissions: EVERYONE, is_everyone: true },
        { id: 11, name: 'mod', position: 2, permissions: Permissions.KICK_MEMBERS | Permissions.MANAGE_MESSAGES, is_everyone: false },
        { id: 12, name: 'vip', position: 1, permissions: 0, is_everyone: false },
    ],
    members: [
        { user_id: 1, name: 'Dono', role_ids: [10], is_owner: true },
        { user_id: 2, name: 'Mod', role_ids: [10, 11], is_owner: false },
        { user_id: 3, name: 'Vip', role_ids: [10, 12], is_owner: false },
        { user_id: 4, name: 'Comum', role_ids: [10], is_owner: false },
    ],
};

const [OWNER, MOD, VIP, PLAIN] = SERVER.members;

describe('permissões: qual botão aparece', () => {
    it('os bits batem com a tabela do CONTRATO.md', () => {
        expect(Permissions.MOVE_MEMBERS).toBe(131072);
        expect(Permissions.ALL).toBe(262143);
        expect(EVERYONE).toBe(31552);
        expect(Permissions.has(EVERYONE, Permissions.SPEAK)).toBe(true);
        expect(Permissions.has(EVERYONE, Permissions.KICK_MEMBERS)).toBe(false);
    });

    it('administrador tem tudo, mesmo com o bit específico apagado', () => {
        expect(Permissions.has(Permissions.ADMINISTRATOR, Permissions.BAN_MEMBERS)).toBe(true);
    });

    it('só se mexe em quem está abaixo, e o dono está acima de todos', () => {
        expect(Permissions.topPosition(SERVER, MOD)).toBe(2);
        expect(Permissions.topPosition(SERVER, PLAIN)).toBe(0);
        expect(Permissions.outranks(SERVER, MOD, VIP)).toBe(true);
        expect(Permissions.outranks(SERVER, OWNER, MOD)).toBe(true);
        expect(Permissions.outranks(SERVER, VIP, MOD)).toBe(false);
        expect(Permissions.outranks(SERVER, MOD, MOD), 'ninguém está acima de si mesmo').toBe(false);
        expect(Permissions.outranks(SERVER, MOD, OWNER)).toBe(false);
    });
});

describe('modo servidor com a API de mentira', () => {
    const invokes: { command: string; args: { muted?: boolean } }[] = [];
    const calls: { method: string; path: string; body: unknown }[] = [];
    const toasts: string[] = [];
    const responses = new Map<string, unknown>();
    const quiet = { here() { return this; }, joining() { return this; }, leaving() { return this; }, listen() { return this; } };
    const heard: Record<string, (payload: unknown) => void> = {};
    const echoSteps: string[] = [];
    const subscription = (name: string) => ({
        listen(event: string, handler: (payload: unknown) => void) {
            heard[`${name} ${event}`] = handler;

            return this;
        },
        stopListening(event: string) {
            echoSteps.push(`stop ${name} ${event}`);

            return this;
        },
    });
    const track = { enabled: true, stop() {} };
    const micSteps: string[] = [];
    let app: App;
    let hub: Hub;
    let voice: Voice;
    let servers = [
        { id: 2, name: 'Jogatina', owner_id: 1, last_accessed_at: '2026-09-12T20:00:00Z' },
        { id: 3, name: 'Trampo', owner_id: 9, last_accessed_at: null },
    ];

    const tree = (id: number, name: string) => ({
        id,
        name,
        owner_id: 1,
        invite_code: `convite-${id}`,
        me: { user_id: 1, permissions: Permissions.ALL, top_position: 2147483647 },
        roles: [],
        channels: [
            { id: `text-${id}`, name: 'geral', type: 'text', topic: null, position: 0, permissions: EVERYONE },
            { id: `voice-${id}`, name: 'Geral', type: 'voice', topic: null, position: 1, permissions: EVERYONE },
            { id: `locked-${id}`, name: 'Fechada', type: 'voice', topic: null, position: 2, permissions: Permissions.VIEW_CHANNEL },
        ],
        members: [{ user_id: 1, name: 'Edsu', nickname: null, role_ids: [], server_mute: false, server_deaf: false, is_owner: true }],
        voice: { [`voice-${id}`]: [{ user_id: 40, name: 'Fulano', sources: ['mic'] }] },
    });
    const tokenCalls = () => calls.filter(call => call.path.includes('/voice/token'));
    const FULANO = { user_id: 40, name: 'Fulano', sources: ['mic'] };
    const MINE = { user_id: 1, name: 'Edsu', sources: [], muted: false };
    const OTHER_VOICE = { id: 'voice-outra', name: 'Outra', type: 'voice', topic: null, position: 3, permissions: EVERYONE };
    const voiceList = (channelId: string) => hub.store.state.tree?.voice[channelId] ?? [];
    const holdEntrance = () => {
        const usual = app.media.enterRoom;
        const doors: { open(): void; fail(reason: Error): void }[] = [];

        hub.tree = { ...hub.tree!, channels: [...tree(2, 'Jogatina').channels, OTHER_VOICE], voice: { 'voice-2': [FULANO] } };
        hub.publish();
        app.media.enterRoom = (sfu, identity, alongside) => new Promise((resolve, reject) => doors.push({
            open: () => resolve(usual(sfu, identity, (joined: unknown) => {
                app.media.sfu.peers.set('me', { peerId: 'me', self: true, name: 'Edsu', producers: [] });
                app.media.sfu.peers.set('bia', { peerId: 'bia', userId: 'user:7', name: 'Bia', producers: [{ producerId: 'bia-mic', source: 'mic', paused: true }] });

                return alongside(joined);
            })),
            fail: reject,
        }));

        return {
            doors,
            reached: (count: number) => vi.waitFor(() => expect(doors.length).toBe(count)),
            restore: async () => {
                await voice.leave();
                app.media.enterRoom = usual;
            },
        };
    };
    const notFound = () => {
        throw Object.assign(new Error('Servidor não encontrado.'), { status: 404 });
    };

    beforeAll(() => {
        window.__TAURI__ = {
            core: {
                invoke: async (command, args) => {
                    invokes.push({ command, args });

                    return command === 'sfu_offer' ? { ssrc: 7, payloadType: 111 } : null;
                },
            },
            event: { listen: async () => () => null },
        };

        app = new App();
        hub = app.hub;
        voice = hub.voice;
        app.toast = message => toasts.push(message);
        hub.api.request = async (method: string, path: string, body: unknown) => {
            calls.push({ method, path, body });

            const answer = responses.get(`${method} ${path}`);

            return typeof answer === 'function' ? answer(body) : answer ?? null;
        };
        hub.user = { id: 1, name: 'Edsu' };
        hub.config = {};
        hub.echo = { private: subscription, join: () => quiet, leave: (name: string) => echoSteps.push(`leave ${name}`), disconnect() {} };

        responses.set('GET /api/servers', () => servers);
        responses.set('GET /api/channels/voice-2/messages', []);
        responses.set('GET /api/channels/voice-outra/messages', []);
        responses.set('GET /api/servers/2', tree(2, 'Jogatina'));
        responses.set('GET /api/servers/5', tree(5, 'Nova'));
        responses.set('GET /api/channels/text-2/messages', []);
        responses.set('GET /api/channels/text-5/messages', []);
        responses.set('POST /api/servers', ({ name }: { name: string }) => {
            servers = [{ id: 5, name, owner_id: 1, last_accessed_at: null }, ...servers];

            return { id: 5, name, owner_id: 1 };
        });
    });

    it('logado e sem servidor aberto, fica na Home: nada abre sozinho', async () => {
        await hub.open();

        expect(app.store.state.screen).toBe('hub');
        expect(hub.tree, 'o primeiro servidor não abre sozinho').toBeNull();
        expect(hub.store.state.home).toBe(true);
        expect(hub.store.state.servers.map(item => item.name), 'na ordem em que o Laravel mandou').toEqual(['Jogatina', 'Trampo']);
    });

    it('abrir uma das últimas salas abre o primeiro canal de texto e mostra quem está na voz, sem entrar nela', async () => {
        await hub.openServer(2);

        expect(hub.store.state.tree?.id).toBe(2);
        expect(hub.store.state.home).toBe(false);
        expect(hub.store.state.channel?.id).toBe('text-2');
        expect(hub.store.state.tree?.voice['voice-2'][0].name, 'quem está em cada voz aparece').toBe('Fulano');
        expect(voice.channel, 'abrir não é entrar na voz').toBeNull();
        expect(tokenCalls().length).toBe(0);
    });

    it('a Home não fecha o servidor: a voz de um canal dele continuaria de pé', () => {
        hub.showHome();

        expect(hub.store.state.home).toBe(true);
        expect(hub.tree?.id).toBe(2);
    });

    it('trocar de servidor não passa pela Home no caminho', async () => {
        await hub.openServer(2);
        const states: { home: boolean; homeLit: boolean }[] = [];
        const unsubscribe = hub.store.subscribe(() => {
            const { home, tree, treeLoading } = hub.store.state;

            states.push({ home, homeLit: home || (! tree && ! treeLoading) });
        });

        await hub.openServer(5);
        unsubscribe();

        expect(hub.store.state.tree?.id).toBe(5);
        expect(states.some(state => state.home), 'a Home não aparece').toBe(false);
        expect(states.some(state => state.homeLit), 'nem o ícone da Home acende').toBe(false);
    });

    it('criar sala abre a sala nova sem entrar na voz e mostra o convite', async () => {
        await hub.closeServer();
        await hub.createServer('Nova');

        expect(calls.some(call => call.method === 'POST' && call.path === '/api/servers')).toBe(true);
        expect(hub.store.state.tree?.id, 'criar abre a sala nova').toBe(5);
        expect(voice.channel, 'e não entra na voz').toBeNull();
        expect(hub.store.state.inviteBanner).toBe(true);
        expect(hub.store.state.tree?.invite_code).toBe('convite-5');
        expect(tokenCalls().length).toBe(0);
    });

    it('apagado por outra pessoa, o servidor sai da tela e da lista', async () => {
        responses.set('GET /api/servers/5', notFound);
        servers = servers.filter(item => item.id !== 5);

        await hub.attempt(() => hub.openServer(5));

        expect(hub.store.state.tree).toBeNull();
        expect(hub.store.state.home).toBe(true);
        expect(hub.store.state.servers.some(item => item.id === 5), 'a lista vem de novo, sem ele').toBe(false);
        expect(toasts.at(-1)).toBe('Servidor não encontrado.');
    });

    it('trocar de outro servidor para um apagado não prende o carregamento', async () => {
        responses.set('GET /api/servers/9', notFound);
        servers = [...servers, { id: 9, name: 'Sumiu', owner_id: 1, last_accessed_at: null }];
        await hub.loadServers(2);
        servers = servers.filter(item => item.id !== 9);

        await hub.attempt(() => hub.openServer(9));

        expect(hub.store.state.treeLoading).toBe(false);
        expect(hub.store.state.tree).toBeNull();
    });

    it('com "Silenciar ao entrar" marcado, entrar na voz liga o mic mutado, e mutado antes de publicar', async () => {
        expect(voice.store.state.preferences.muteOnJoin, 'o padrão é entrar com o microfone aberto').toBe(false);
        await voice.setPreference('muteOnJoin', true);

        Object.defineProperty(window.navigator, 'mediaDevices', { value: { getUserMedia: async () => ({ getAudioTracks: () => [track] }) }, configurable: true });
        app.media.tearDown = async () => {
            app.media.sfu = null;
        };
        app.media.enterRoom = async (sfu, identity, alongside) => {
            app.media.sfu = {
                peers: new Map(),
                producers: new Map(),
                produce: async (produced: { enabled: boolean }, source: string) => {
                    micSteps.push(`produce:${source}:${produced.enabled ? 'on' : 'off'}`);

                    return { id: `${source}-1` };
                },
                pauseProducer: async (producerId: string) => micSteps.push(`pause:${producerId}`),
                resumeProducer: async (producerId: string) => micSteps.push(`resume:${producerId}`),
                closeProducer: async () => null,
                disconnect() {},
                request: async () => ({ producerId: 'mic-native', ip: '127.0.0.1', port: 40000 }),
            };
            await alongside({ can: ['speak'] });
        };

        await hub.openServer(2);
        voice.native = () => false;
        await hub.openChannel(hub.tree!.channels[1]);

        expect(hub.store.state.stageOpen, 'entrar na voz abre as transmissões ao lado').toBe(true);
        expect(voice.store.state.muted).toBe(true);
        expect(voice.store.state.joining, 'o carregamento termina junto com a entrada').toBe(false);
        expect(micSteps, 'a trilha já sobe desligada, e o producer pausa no servidor').toEqual(['produce:mic:off', 'pause:mic-1']);
    });

    it('desmutar retoma no servidor e religa a trilha', async () => {
        await voice.toggleMute();

        expect(voice.store.state.muted).toBe(false);
        expect(micSteps.at(-1)).toBe('resume:mic-1');
        expect(track.enabled).toBe(true);
    });

    it('sair da voz fecha o painel das transmissões e a sala focada', async () => {
        hub.setFocusedRoom(true);
        await voice.leave();

        expect(hub.store.state.stageOpen).toBe(false);
        expect(hub.store.state.focusedRoom).toBe(false);
    });

    it('a voz tem chat: abre ao entrar, conta o que chega com o painel fechado e fecha ao sair sem largar o canal do Reverb', async () => {
        const voiceChannel = hub.tree!.channels[1];
        const message = (id: number, userId: number) => ({ id, channel_id: 'voice-2', type: 'user', body: `m${id}`, files: [], reply_to: null, user: { id: userId, name: 'Alguém' } });

        responses.set('GET /api/channels/voice-2/messages', [message(1, 7)]);
        echoSteps.length = 0;
        await hub.openChannel(voiceChannel);
        await new Promise(resolve => setTimeout(resolve, 0));

        expect(hub.voiceChat.store.state.messages.map(item => item.id), 'entrar na voz abre o chat dela').toEqual([1]);
        expect(hub.chat.store.state.channel?.id, 'o chat de texto continua no canal dele').toBe('text-2');
        expect(hub.store.state.stageChat, 'o painel começa fechado').toBeNull();

        heard['channel.voice-2 .MessageSent']({ message: message(2, 7) });
        heard['channel.voice-2 .MessageSent']({ message: message(3, 1) });
        expect(hub.voiceChat.store.state.unread, 'a minha própria mensagem não conta').toBe(1);
        expect(hub.chat.store.state.messages, 'mensagem da voz não cai no canal de texto').toEqual([]);

        hub.setStageChat('voice');
        expect(hub.voiceChat.store.state.unread, 'abrir o painel zera o indicador').toBe(0);
        heard['channel.voice-2 .MessageSent']({ message: message(4, 7) });
        expect(hub.voiceChat.store.state.unread, 'com o painel aberto nada acumula').toBe(0);

        hub.showStage(false);
        heard['channel.voice-2 .MessageSent']({ message: message(5, 7) });
        expect(hub.voiceChat.store.state.unread, 'de volta ao chat de texto o painel da voz não está à vista').toBe(1);

        await voice.leave();

        expect(hub.voiceChat.store.state.channel).toBeNull();
        expect(hub.store.state.stageChat).toBeNull();
        expect(echoSteps).toContain('stop channel.voice-2 .MessageSent');
        expect(echoSteps, 'o mesmo canal do Reverb carrega o VoiceStateUpdated: sair dele apagaria quem está na voz').not.toContain('leave channel.voice-2');

        responses.set('GET /api/channels/voice-2/messages', []);
    });

    it('com "Silenciar ao entrar" desligado, que é o padrão, a pessoa entra falando', async () => {
        await voice.setPreference('muteOnJoin', false);
        micSteps.length = 0;
        track.enabled = true;
        await voice.join(hub.tree!.channels[1]);

        expect(voice.store.state.muted).toBe(false);
        expect(micSteps).toEqual(['produce:mic:on']);

        await voice.leave();
        await voice.setPreference('muteOnJoin', true);
    });

    it('no Linux o Rust cala antes de o RTP ter para onde ir', async () => {
        invokes.length = 0;
        voice.native = () => true;
        await voice.join(hub.tree!.channels[1]);

        const order = invokes
            .filter(({ command }) => command !== 'log_line')
            .map(({ command, args }) => (command === 'set_voice_muted' ? `muted:${args.muted}` : command));

        expect(voice.muted).toBe(true);
        expect(order.indexOf('start_voice'), `ordem: ${order}`).toBeLessThan(order.indexOf('muted:true'));
        expect(order.indexOf('muted:true'), 'calado antes de o RTP ter para onde ir').toBeLessThan(order.indexOf('use_sfu'));
        expect(order, 'ninguém desmuta sozinho').not.toContain('muted:false');

        await voice.leave();
        voice.native = () => false;
    });

    it('trocar de canal de voz tira a voz velha antes, e as transmissões da nova ficam abertas', async () => {
        await hub.openChannel(hub.tree!.channels[1]);
        await hub.openChannel({ id: 'voice-outra', name: 'Outra', type: 'voice', permissions: EVERYONE });

        expect(voice.channel?.id).toBe('voice-outra');
        expect(hub.store.state.stageOpen, 'trocar de voz não fecha as transmissões da voz nova').toBe(true);
    });

    it('na sala focada, escolher um canal de texto troca o chat sem sair da sala', async () => {
        hub.setFocusedRoom(true);
        await hub.openChannel(hub.tree!.channels[0]);

        expect(hub.store.state.focusedRoom).toBe(true);

        await voice.leave();
    });

    it('o microfone autorizado depois de sair da voz não fica capturando', async () => {
        const lateTrack = { enabled: true, stopped: false, stop() { this.stopped = true; } };
        const grantMic: (() => void)[] = [];

        navigator.mediaDevices.getUserMedia = () => new Promise(resolve => grantMic.push(() => resolve({ getAudioTracks: () => [lateTrack] })));

        const joiningLate = voice.join(hub.tree!.channels[1]);

        await new Promise(resolve => setTimeout(resolve, 0));
        await voice.leave();
        grantMic[0]();
        await joiningLate;

        expect(lateTrack.stopped).toBe(true);
        expect(voice.micTrack).toBeNull();

        navigator.mediaDevices.getUserMedia = async () => ({ getAudioTracks: () => [track] });
    });

    it('o clique no canal de voz vale na hora: a voz, o palco e a própria pessoa na lista, antes de a conexão começar', async () => {
        const { doors, reached, restore } = holdEntrance();

        await voice.setPreference('muteOnJoin', false);

        const entering = hub.openChannel(hub.tree!.channels[1]);

        expect(voice.store.state.channel?.id, 'sem esperar token, SFU nem microfone').toBe('voice-2');
        expect(voice.store.state.joining, 'a barra de voz mostra "Conectando…"').toBe(true);
        expect(hub.store.state.stageOpen).toBe(true);
        expect(voiceList('voice-2')).toEqual([MINE, FULANO]);
        expect(doors.length, 'a conexão vem depois, por trás').toBe(0);

        await reached(1);
        doors[0].open();
        await entering;

        expect(voice.store.state.joining).toBe(false);
        expect(voiceList('voice-2'), 'conectado, a lista passa a ser a do SFU').toEqual([MINE, { user_id: 7, name: 'Bia', sources: ['mic'], muted: true }]);

        await voice.leave();

        expect(voiceList('voice-2').some(person => person.user_id === 1), 'sair tira da lista na hora').toBe(false);

        await restore();
    });

    it('o VoiceStateUpdated sobre mim não duplica a entrada otimista, e o left atrasado da sessão anterior não a apaga', async () => {
        const { doors, reached, restore } = holdEntrance();
        const entering = hub.openChannel(hub.tree!.channels[1]);
        const voiceState = heard['channel.voice-2 .VoiceStateUpdated'];

        await reached(1);
        voiceState({ channel_id: 'voice-2', user_id: 1, name: 'Edsu', event: 'joined' });
        expect(voiceList('voice-2')).toEqual([MINE, FULANO]);

        voiceState({ channel_id: 'voice-2', user_id: 1, name: 'Edsu', event: 'left' });
        expect(voiceList('voice-2'), 'estou entrando: o left é da sessão de antes').toEqual([MINE, FULANO]);

        await restore();
        doors[0].fail(new Error('entrada abandonada'));
        await entering;

        voiceState({ channel_id: 'voice-2', user_id: 1, name: 'Edsu', event: 'left' });
        expect(voiceList('voice-2'), 'fora da voz o left vale').toEqual([FULANO]);
    });

    it('trocar de canal de voz tira a pessoa do antigo e põe no novo na hora, e a saída do antigo não apaga o estado do novo', async () => {
        const { doors, reached, restore } = holdEntrance();
        const entering = hub.openChannel(hub.tree!.channels[1]);

        await reached(1);
        doors[0].open();
        await entering;
        hub.setFocusedRoom(true);
        voice.cameraProducerId = 'camera-1';
        voice.publish();

        const switching = hub.openChannel(OTHER_VOICE);

        expect(voice.store.state.channel?.id).toBe('voice-outra');
        expect(voice.store.state.joining).toBe(true);
        expect(voiceList('voice-2').some(person => person.user_id === 1), 'some do canal antigo na hora').toBe(false);
        expect(voiceList('voice-outra'), 'a lista do canal novo não herda quem estava no SFU do antigo').toEqual([MINE]);

        await reached(2);

        expect(voice.store.state.joining, 'a saída do canal antigo não desliga o "Conectando…" do novo').toBe(true);
        expect(hub.store.state.stageOpen, 'nem fecha o palco').toBe(true);
        expect(hub.store.state.focusedRoom, 'nem a sala focada').toBe(true);
        expect(voice.store.state.cameraOn, 'mas a câmera que ficou no canal antigo apaga o botão').toBe(false);

        doors[1].open();
        await switching;

        expect(voice.store.state.joining).toBe(false);
        expect(voiceList('voice-outra').map(person => person.name)).toEqual(['Edsu', 'Bia']);

        await restore();
    });

    it('a entrada que falha tira a pessoa da lista do canal', async () => {
        const { doors, reached, restore } = holdEntrance();
        const entering = hub.openChannel(hub.tree!.channels[1]);

        await reached(1);
        expect(voiceList('voice-2')).toEqual([MINE, FULANO]);

        doors[0].fail(new Error('SFU fora do ar'));
        await entering;

        expect(toasts.at(-1)).toBe('não deu para entrar na voz: SFU fora do ar');
        expect(voice.channel).toBeNull();
        expect(voice.store.state.joining).toBe(false);
        expect(voiceList('voice-2'), 'ninguém mandaria o left de quem nunca chegou ao SFU').toEqual([FULANO]);

        await restore();
    });

    it('clicar em outro canal no meio da entrada não deixa fantasma no primeiro, e a recusa atrasada dele não derruba o segundo', async () => {
        const { doors, reached, restore } = holdEntrance();
        const abandoned = hub.openChannel(hub.tree!.channels[1]);

        await reached(1);

        const chosen = hub.openChannel(OTHER_VOICE);

        expect(voiceList('voice-2')).toEqual([FULANO]);
        expect(voiceList('voice-outra')).toEqual([MINE]);

        responses.set('POST /api/channels/voice-2/voice/token', () => {
            throw Object.assign(new Error('O canal está cheio.'), { status: 403 });
        });
        await expect(voice.identity(hub.tree!.channels[1])).rejects.toThrow();
        responses.delete('POST /api/channels/voice-2/voice/token');
        doors[0].fail(new Error('entrada abandonada'));
        await abandoned;

        expect(voice.channel?.id, 'a recusa do canal abandonado não tira a pessoa do canal escolhido').toBe('voice-outra');

        await reached(2);
        doors[1].open();
        await chosen;

        expect(voice.store.state.channel?.id).toBe('voice-outra');
        expect(voice.store.state.joining).toBe(false);
        expect(voiceList('voice-2')).toEqual([FULANO]);
        expect(voiceList('voice-outra').map(person => person.name)).toEqual(['Edsu', 'Bia']);

        await restore();
    });

    it('o ícone de mudo da própria pessoa na lista do canal segue o padrão da entrada, o botão e o mudo do servidor', async () => {
        const { doors, reached, restore } = holdEntrance();
        const mine = () => voiceList('voice-2').find(person => person.user_id === 1);

        await voice.setPreference('muteOnJoin', true);

        const entering = hub.openChannel(hub.tree!.channels[1]);

        expect(mine()?.muted, 'com "Silenciar ao entrar", já aparece mutada no clique').toBe(true);

        await reached(1);
        doors[0].open();
        await entering;
        expect(mine()?.muted).toBe(true);

        await voice.toggleMute();
        expect(mine()?.muted, 'desmutei: o ícone some sem esperar ninguém').toBe(false);

        await voice.toggleMute();
        expect(mine()?.muted).toBe(true);

        await voice.toggleMute();
        await voice.applyServerMute(true);
        expect(mine()?.muted, 'mudo do servidor').toBe(true);

        await voice.applyServerMute(false);
        expect(mine()?.muted).toBe(false);

        await restore();
    });

    it('sem CONNECT no canal: aviso, e nada de pedir token', async () => {
        const before = tokenCalls().length;

        await hub.openChannel(hub.tree!.channels[2]);

        expect(voice.channel).toBeNull();
        expect(toasts.at(-1)).toBe('você não pode entrar neste canal de voz');
        expect(tokenCalls().length).toBe(before);
    });

    it('token recusado (canal cheio, permissão tirada) sai da voz e diz por quê', async () => {
        responses.set('POST /api/channels/voice-2/voice/token', () => {
            throw Object.assign(new Error('O canal está cheio.'), { status: 403 });
        });
        voice.channel = hub.tree!.channels[1];

        await expect(voice.identity(hub.tree!.channels[1])).rejects.toThrow();

        expect(toasts.at(-1)).toBe('não deu para entrar na voz: O canal está cheio.');
        expect(voice.channel, 'recusado, sai da voz').toBeNull();
    });

    it('o menu de um membro só oferece o que a hierarquia e os bits deixam', () => {
        hub.tree = { ...SERVER, id: 'hierarquia', voice: { sala: [{ user_id: 3, name: 'Vip' }] }, me: { permissions: Permissions.KICK_MEMBERS | Permissions.MOVE_MEMBERS, top_position: 2 } };
        hub.user = { id: 2, name: 'Mod' };

        expect(hub.memberActions(VIP).kick, 'o mod expulsa quem está abaixo').toBe(true);
        expect(hub.memberActions(VIP).disconnect, 'e desconecta da voz quem está numa').toBe(true);
        expect(hub.memberActions(PLAIN).disconnect, 'quem não está em voz não tem o que desconectar').toBe(false);
        expect(hub.memberActions(VIP).ban, 'sem o bit, sem banir').toBe(false);
        expect(hub.memberActions(OWNER).kick, 'ninguém mexe no dono').toBe(false);
        expect(hub.memberActions(MOD).nickname, 'o próprio apelido sempre').toBe(true);
        expect(hub.memberActions(MOD).kick, 'ninguém se expulsa').toBe(false);
    });

    it('expulsar pergunta antes; recusou, nada vai para a API', async () => {
        app.confirm = async () => false;
        calls.length = 0;

        await hub.kickMember(VIP);

        expect(calls.length).toBe(0);
    });

    it('o Esc fecha de cima para baixo: menu, editor de cargo, modal', () => {
        hub.store.set({ modal: { type: 'settings' }, roleEditor: { role: null }, memberMenu: { userId: 3, x: 0, y: 0 } });

        expect(hub.closeTopmost()).toBe(true);
        expect(hub.store.state.memberMenu).toBeNull();
        expect(hub.store.state.roleEditor).toBeTruthy();

        hub.closeTopmost();
        expect(hub.store.state.roleEditor).toBeNull();
        expect(hub.store.state.modal).toBeTruthy();

        hub.closeTopmost();
        expect(hub.store.state.modal).toBeNull();
        expect(hub.closeTopmost()).toBe(false);
    });

    it('o @everyone não sobe: ele é sempre o último, e a API recusaria', () => {
        hub.user = { id: 1, name: 'Edsu' };
        hub.tree = {
            ...tree(2, 'Jogatina'),
            roles: [
                { id: 20, name: '@everyone', position: 0, permissions: EVERYONE, is_everyone: true },
                { id: 21, name: 'Moderador', position: 1, permissions: 0, is_everyone: false },
            ],
        };

        const rows = hub.settings.roleRows();

        expect(rows.find(row => row.role.is_everyone)?.up, '@everyone não ganha ▲').toBeNull();
        expect(rows.find(row => row.role.name === 'Moderador')?.down, 'e ninguém desce para baixo dele').toBeNull();
    });

    it('renomeado por alguém, o trilho e as últimas salas trocam o nome junto com a árvore', async () => {
        hub.tree = null;
        hub.servers = [{ id: 2, name: 'Jogatina', owner_id: 1, last_accessed_at: null }];
        responses.set('GET /api/servers/2', tree(2, 'Jogatina renomeada'));

        await hub.openServer(2);

        expect(hub.store.state.servers[0].name).toBe('Jogatina renomeada');
    });

    it('formulário fora da regra vira aviso do próprio app, onde a tela já mostra erro, e nada vai para a API', async () => {
        calls.length = 0;

        expect(await hub.login('', 'segredo')).toBe(false);
        expect(hub.store.state.loginFieldErrors).toEqual({ email: 'Digite o e-mail.' });
        expect(hub.store.state.loginError, 'erro de campo fica embaixo do campo, não na linha geral').toBe('');
        expect(await hub.login('edsu.example.com', '')).toBe(false);
        expect(hub.store.state.loginFieldErrors, 'cada campo com o seu erro, de uma vez').toEqual({ email: 'Esse e-mail não parece válido.', password: 'Digite a senha.' });
        hub.clearLoginFieldError('email');
        expect(hub.store.state.loginFieldErrors, 'editar um campo apaga só o erro dele').toEqual({ password: 'Digite a senha.' });
        expect(await hub.register('edsu@example.com', '1234567')).toBe(false);
        expect(hub.store.state.loginFieldErrors).toEqual({ password: 'A senha precisa ter pelo menos 8 caracteres.' });
        expect(hub.store.state.loginBusy).toBe(false);

        await hub.attempt(() => hub.createServer('   '));
        expect(toasts.at(-1)).toBe('Dê um nome ao servidor.');
        await hub.attempt(() => hub.joinInvite(''));
        expect(toasts.at(-1)).toBe('Cole o código do convite.');
        expect(await hub.friends.request('fulano')).toBe(false);
        expect(toasts.at(-1)).toBe('Esse e-mail não parece válido.');
        await hub.settings.saveRole(null, { name: ' ', color: '#8a7cf5', permissions: 0 });
        expect(toasts.at(-1)).toBe('Dê um nome ao cargo.');
        await hub.settings.saveChannel(null, { name: 'sala', type: 'voice', topic: '', limit: '150' });
        expect(toasts.at(-1)).toBe('O limite de pessoas vai de 1 a 99. Vazio é sem limite.');

        expect(calls.filter(call => call.method !== 'GET'), 'nenhum formulário recusado chega à API').toEqual([]);
    });

    it('o 422 do Laravel cai no campo de cada chave; o que não é de campo fica na linha geral', async () => {
        const refused = (status: number, message: string, errors: Record<string, string[]> = {}) => () => {
            throw Object.assign(new Error(message), { status, errors });
        };

        responses.set('POST /api/auth/register', refused(422, 'Esse e-mail já tem conta.', { email: ['Esse e-mail já tem conta.'], password: ['Senha fraca.'] }));
        expect(await hub.register(' ana@example.com ', 'segredo123')).toBe(false);
        expect(calls.at(-1), 'o cadastro vai sem apelido').toEqual({ method: 'POST', path: '/api/auth/register', body: { email: 'ana@example.com', password: 'segredo123', device: 'app' } });
        expect(hub.store.state.loginFieldErrors).toEqual({ email: 'Esse e-mail já tem conta.', password: 'Senha fraca.' });
        expect(hub.store.state.loginError).toBe('');

        responses.set('POST /api/auth/login', refused(429, 'Muitas tentativas.'));
        expect(await hub.login('ana@example.com', 'segredo123')).toBe(false);
        expect(hub.store.state.loginFieldErrors).toEqual({});
        expect(hub.store.state.loginError).toBe('Muitas tentativas.');
    });

    it('apelido não confirmado: o PATCH /api/me troca a conta e o modal some; recusado, o erro fica no campo', async () => {
        const unconfirmed = { id: 1, name: 'edsu4821', nickname_confirmed: false };

        hub.user = unconfirmed;
        hub.publish();
        calls.length = 0;

        await hub.confirmNickname('ed');
        expect(hub.store.state.nicknameError).toBe('O apelido precisa ter de 3 a 32 caracteres.');
        expect(calls, 'recusado aqui, nada vai para a API').toEqual([]);

        responses.set('PATCH /api/me', () => {
            throw Object.assign(new Error('Esse apelido já é de alguém.'), { status: 422, errors: { name: ['Esse apelido já é de alguém.'] } });
        });
        await hub.confirmNickname('edsu');
        expect(hub.store.state.nicknameError).toBe('Esse apelido já é de alguém.');
        expect(hub.store.state.user?.nickname_confirmed, 'o modal continua').toBe(false);

        responses.set('PATCH /api/me', ({ name }: { name: string }) => ({ id: 1, name, nickname_confirmed: true }));
        await hub.confirmNickname('Edsu');
        expect(calls.at(-1)).toEqual({ method: 'PATCH', path: '/api/me', body: { name: 'Edsu' } });
        expect(hub.user).toEqual({ id: 1, name: 'Edsu', nickname_confirmed: true });
        expect(hub.store.state.user?.nickname_confirmed, 'o modal some').toBe(true);
        expect(hub.store.state.nicknameError).toBe('');
        expect(hub.store.state.nicknameBusy).toBe(false);

        hub.user = unconfirmed;
        hub.publish();
        responses.set('PATCH /api/me', () => {
            throw Object.assign(new Error('Apelido já confirmado.'), { status: 403, errors: {} });
        });
        responses.set('GET /api/me', { id: 1, name: 'Edsu', nickname_confirmed: true });
        await hub.confirmNickname('Edsu');
        expect(hub.store.state.user?.nickname_confirmed, 'já confirmado em outro lugar: recarrega o /api/me e fecha').toBe(true);
        expect(hub.store.state.nicknameError).toBe('');
    });

    it('a foto de perfil: acima de 2 MB nem sai do app, e a API é quem diz qual foto vale depois', async () => {
        hub.user = { id: 1, name: 'Edsu' };
        hub.publish();
        calls.length = 0;
        toasts.length = 0;

        await hub.uploadAvatar(new File([new Uint8Array(2 * 1024 * 1024 + 1)], 'grande.png', { type: 'image/png' }));
        expect(calls, 'o teto é conferido antes da rede').toEqual([]);
        expect(toasts.at(-1)).toBe('a foto precisa ter menos de 2 MB');

        responses.set('POST /api/me/avatar', { id: 1, name: 'Edsu', avatar_url: 'https://bucket/eu.png', avatar_uploaded: true });
        await hub.uploadAvatar(new File([new Uint8Array(16)], 'eu.png', { type: 'image/png' }));
        expect(calls.at(-1)?.path).toBe('/api/me/avatar');
        expect(hub.store.state.user?.avatar_url).toBe('https://bucket/eu.png');
        expect(hub.store.state.user?.avatar_uploaded).toBe(true);

        responses.set('DELETE /api/me/avatar', { id: 1, name: 'Edsu', avatar_url: 'https://google/eu.png', avatar_uploaded: false });
        await hub.removeAvatar();
        expect(hub.store.state.user?.avatar_url, 'sem a foto enviada volta a valer a do Google').toBe('https://google/eu.png');
        expect(hub.store.state.user?.avatar_uploaded).toBe(false);
    });

    it('no canal em que estou, a lista da voz é a do SFU: quem foi expulso sai na hora', () => {
        voice.channel = hub.tree!.channels[1];
        app.media.sfu = {
            peers: new Map([
                ['me', { peerId: 'me', self: true, name: 'Edsu', producers: [] }],
                ['bia', { peerId: 'bia', userId: 'user:7', name: 'Bia', producers: [{ producerId: 's', source: 'screen' }] }],
                ['convidado', { peerId: 'convidado', userId: 'guest:x', name: 'Sem conta', producers: [] }],
            ]),
        };

        hub.syncVoiceSources();

        expect(hub.store.state.tree?.voice['voice-2'].map(person => person.name), 'o Fulano da presença velha sai, a Bia do SFU entra').toEqual(['Edsu', 'Bia']);
        expect(hub.store.state.tree?.voice['voice-2'][1].sources).toEqual(['screen']);
    });

    it('o VoiceStateUpdated que chega atrasado pela fila do Laravel não apaga o AO VIVO que o SFU já mostrou', () => {
        const voiceHandlers: Record<string, (payload: unknown) => void> = {};

        hub.echo = {
            private: () => ({
                listen(name: string, handler: (payload: unknown) => void) {
                    voiceHandlers[name] = handler;

                    return this;
                },
            }),
            join: () => quiet,
            leave() {},
            disconnect() {},
        };
        hub.voiceChannels.clear();
        hub.subscribeVoiceStates();
        voiceHandlers['.VoiceStateUpdated']({ channel_id: 'voice-2', user_id: 7, name: 'Bia', event: 'joined' });

        expect(hub.store.state.tree?.voice['voice-2'].find(person => person.name === 'Bia')?.sources).toEqual(['screen']);

        voice.channel = null;
    });

    it('uma sala abandonada que só falha depois não derruba a sessão que veio em seguida', async () => {
        const tornDown: unknown[] = [];
        let failJoin: (reason: Error) => void = () => {};

        app.media.enterRoom = sfu => {
            app.media.sfu = sfu;

            return new Promise((resolve, reject) => {
                failJoin = reject;
            });
        };
        app.media.tearDown = async () => {
            tornDown.push(app.media.sfu);
            app.media.sfu = null;
        };

        const abandoned = app.connect('sala-velha');
        const newer = { peers: new Map() };

        app.media.sfu = newer;
        failJoin(new Error('tempo esgotado'));
        await abandoned;

        expect(app.media.sfu, 'a sessão nova continua').toBe(newer);
        expect(tornDown.length).toBe(0);
    });

    it('a sala por código vai com o nome sem conta, e com o token da sala quando há conta', async () => {
        const identities: unknown[] = [];

        app.media.enterRoom = async (_sfu, identity) => {
            identities.push(await identity());

            return {};
        };
        responses.set('POST /api/rooms/minha-sala/token', { token: 'token-da-sala' });
        calls.length = 0;

        hub.user = null;
        hub.publish();
        app.setEntry({ entryName: 'Visitante' });
        await app.openRoom('minha-sala');
        expect(identities).toEqual([{ room: 'minha-sala', name: 'Visitante', installId: expect.any(String) }]);
        expect(calls, 'sem conta o Laravel fica fora do caminho').toEqual([]);

        hub.user = { id: 1, name: 'Edsu', nickname_confirmed: true };
        hub.publish();
        await app.openRoom('minha-sala');
        expect(identities.at(-1)).toEqual({ token: 'token-da-sala' });
        expect(app.store.state.screen).toBe('room');
    });

    it('com a sala por código aberta, entrar ou sair da conta não esconde a sala', async () => {
        expect(app.store.state.screen).toBe('room');

        await hub.open();
        expect(app.store.state.screen, 'entrar na conta não troca a tela da sala').toBe('room');

        await hub.logout();
        expect(app.store.state.screen, 'sair da conta também não').toBe('room');
    });
});

describe('o tempo real que caiu e voltou', () => {
    const ids = (from: number, to: number) => Array.from({ length: to - from + 1 }, (_, index) => ({ id: from + index }));

    it('a página nova emenda na que já estava na tela, sem buraco e sem o que foi apagado', () => {
        expect(Chat.mergeLatest(ids(1, 5), ids(4, 53)).map(item => item.id), 'encostou no que já havia: fica o mais antigo').toEqual(ids(1, 53).map(item => item.id));
        expect(Chat.mergeLatest(ids(1, 2), ids(100, 149)), 'chegou mais que uma página: sem emenda, só o que veio').toEqual(ids(100, 149));
        expect(Chat.mergeLatest([{ id: 1 }, { id: 2 }, { id: 3 }], [{ id: 1 }, { id: 3 }]), 'página curta é o canal inteiro: o apagado sai').toEqual([{ id: 1 }, { id: 3 }]);
        expect(Chat.mergeLatest(ids(1, 3), []), 'canal esvaziado').toEqual([]);
    });

    it('reconectado, busca de novo os servidores, o canal e a conversa abertos', async () => {
        const app = new App();
        const hub = app.hub;
        const quiet = { here() { return this; }, joining() { return this; }, leaving() { return this; }, listen() { return this; } };
        const responses = new Map<string, unknown>();
        const calls: string[] = [];
        const message = (id: number) => ({ id, channel_id: 'text-2', body: `m${id}`, user: { id: 1, name: 'Edsu' } });

        hub.api.request = async (method: string, path: string) => {
            calls.push(`${method} ${path}`);

            return responses.get(`${method} ${path}`) ?? [];
        };
        hub.user = { id: 1, name: 'Edsu' };
        hub.echo = { private: () => quiet, join: () => quiet, leave() {}, disconnect() {} };
        responses.set('GET /api/servers', [{ id: 2, name: 'Jogatina', owner_id: 1 }]);
        responses.set('GET /api/servers/2', {
            id: 2,
            name: 'Jogatina',
            me: { user_id: 1, permissions: Permissions.ALL, top_position: 1 },
            roles: [],
            members: [],
            voice: {},
            channels: [{ id: 'text-2', name: 'geral', type: 'text', position: 0, permissions: Permissions.ALL }],
        });
        responses.set('GET /api/channels/text-2/messages', [message(1), message(2)]);
        await hub.openServer(2);
        hub.direct.store.set({ person: { id: 9, name: 'Zé' }, messages: [{ id: 30, body: 'oi' }] });

        responses.set('GET /api/channels/text-2/messages', [message(1), message(3)]);
        responses.set('GET /api/dm/9', [{ id: 30, body: 'oi' }, { id: 31, body: 'voltou?' }]);
        calls.length = 0;

        await hub.catchUp();

        expect(hub.chat.store.state.messages.map(item => item.id), 'o 2 foi apagado e o 3 chegou durante a queda').toEqual([1, 3]);
        expect(hub.direct.store.state.messages.map(item => item.id)).toEqual([30, 31]);
        expect(calls).toEqual(expect.arrayContaining(['GET /api/servers', 'GET /api/servers/2', 'GET /api/friends', 'GET /api/dm']));
    });
});

describe('lista de membros: cargo mais alto manda, e quem está fora aparece por último', () => {
    function role(id: number, name: string, position: number, everyone = false): Role {
        return { id, name, color: null, position, permissions: 0, is_everyone: everyone };
    }

    function member(userId: number, name: string, roleIds: number[], nickname: string | null = null): Member {
        return { user_id: userId, name, avatar_url: null, nickname, role_ids: roleIds, server_mute: false, server_deaf: false, is_owner: false };
    }

    const tree = {
        roles: [role(1, '@everyone', 0, true), role(2, 'Moderador', 10), role(3, 'Admin', 20)],
        members: [
            member(10, 'Zeca', [2]),
            member(11, 'Ana', [3, 2]),
            member(12, 'Bia', []),
            member(13, 'Caio', [3]),
            member(14, 'Dora', [2]),
        ],
    } as ServerTree;

    it('agrupa pelo cargo de maior posição, ordena os grupos de cima para baixo e os nomes dentro de cada um', () => {
        const groups = Members.group(tree, new Set([10, 11, 12, 13, 14]));

        expect(groups.map(group => group.label)).toEqual(['Admin', 'Moderador', '@everyone']);
        expect(groups[0].members.map(item => item.name), 'quem tem Admin e Moderador entra só no Admin').toEqual(['Ana', 'Caio']);
        expect(groups[1].members.map(item => item.name)).toEqual(['Dora', 'Zeca']);
        expect(groups[2].members.map(item => item.name), 'sem cargo cai no @everyone').toEqual(['Bia']);
    });

    it('quem não está online sai do grupo do cargo e vira o último grupo', () => {
        const groups = Members.group(tree, new Set([11]));

        expect(groups.map(group => group.label)).toEqual(['Admin', 'Offline']);
        expect(groups[1].members.map(item => item.name)).toEqual(['Bia', 'Caio', 'Dora', 'Zeca']);
    });

    it('sem ninguém offline o grupo Offline não aparece, e o apelido é quem ordena', () => {
        const apelidado = { ...tree, members: [member(20, 'Zeca', [2], 'Alfa'), member(21, 'Ana', [2])] } as ServerTree;
        const groups = Members.group(apelidado, new Set([20, 21]));

        expect(groups.map(group => group.label)).toEqual(['Moderador']);
        expect(groups[0].members.map(item => Members.displayName(item))).toEqual(['Alfa', 'Ana']);
    });
});

describe('cliente da API: um fetch sem prazo segurou a reconexão por quase três minutos', () => {
    const signals: (AbortSignal | undefined)[] = [];
    const answered = { ok: true, status: 200, headers: { get: () => 'application/json' }, json: async () => ({ data: { token: 't' } }) };

    afterEach(() => {
        signals.length = 0;
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
    });

    it('todo pedido leva prazo de 10 s, e o estouro vira aviso em português, não o TimeoutError cru', async () => {
        const controller = new AbortController();
        const timeout = vi.spyOn(AbortSignal, 'timeout').mockReturnValue(controller.signal);

        vi.stubGlobal('fetch', (_url: string, init: RequestInit) => new Promise((_resolve, reject) => {
            init.signal?.addEventListener('abort', () => reject(new DOMException('The operation timed out.', 'TimeoutError')));
        }));

        const token = new ApiClient('http://api').post('/api/rooms/sala/token');

        controller.abort();

        await expect(token).rejects.toThrow('o servidor não respondeu');
        expect(timeout).toHaveBeenCalledWith(ApiClient.REQUEST_TIMEOUT_MS);
        expect(ApiClient.REQUEST_TIMEOUT_MS).toBe(10_000);
    });

    it('recusa do servidor dentro do prazo continua chegando com o status e a mensagem dele', async () => {
        vi.stubGlobal('fetch', async () => ({ ...answered, ok: false, status: 403, json: async () => ({ message: 'Você não pode entrar.' }) }));

        await expect(new ApiClient('http://api').post('/api/rooms/sala/token')).rejects.toMatchObject({ status: 403, message: 'Você não pode entrar.' });
    });

    it('imagem vai sem prazo: em uplink lento o envio passa de 10 s e ainda está andando', async () => {
        vi.stubGlobal('fetch', async (_url: string, init: RequestInit) => {
            signals.push(init.signal ?? undefined);

            return answered;
        });

        const api = new ApiClient('http://api');

        await api.upload('/api/me/avatar', 'avatar', new File(['x'], 'foto.png'));
        expect(await api.post('/api/rooms/sala/token')).toEqual({ token: 't' });

        expect(signals[0], 'FormData sai sem prazo').toBeUndefined();
        expect(signals[1], 'o resto leva o prazo').toBeInstanceOf(AbortSignal);
    });

    it('motor sem AbortSignal.timeout segue sem prazo em vez de quebrar', async () => {
        vi.stubGlobal('AbortSignal', {});
        vi.stubGlobal('fetch', async (_url: string, init: RequestInit) => {
            signals.push(init.signal ?? undefined);

            return answered;
        });

        expect(await new ApiClient('http://api').get('/api/me')).toEqual({ token: 't' });
        expect(signals).toEqual([undefined]);
    });
});

describe('código de sala: a única chave que existe, então sorteio torto é sala adivinhável', () => {
    const SAMPLE_SIZE = 20_000;

    const codes = Array.from({ length: SAMPLE_SIZE }, () => RoomCode.generate());

    it('todo código sorteado tem o tamanho e o formato que o servidor aceita', () => {
        for (const code of codes) {
            expect(code.length, `código com tamanho errado: ${code}`).toBe(RoomCode.LENGTH);
            expect(RoomCode.isValid(code), `código fora do formato aceito pelo servidor: ${code}`).toBe(true);
        }
    });

    it('aceita nome legível e o mínimo; recusa vazio, curto, longo, maiúscula e hífen na ponta', () => {
        expect(RoomCode.isValid(''), 'vazio não é código').toBe(false);
        expect(RoomCode.isValid('sala-do-time'), 'nome legível precisa ser aceito').toBe(true);
        expect(RoomCode.isValid('abc'), 'código mínimo precisa ser aceito').toBe(true);
        expect(RoomCode.isValid('ab'), 'curto demais não é código').toBe(false);
        expect(RoomCode.isValid('a'.repeat(33)), 'longo demais não é código').toBe(false);
        expect(RoomCode.isValid('A'.repeat(RoomCode.LENGTH)), 'maiúscula não é código — o servidor recusa').toBe(false);
        expect(RoomCode.isValid('sala-'), 'hífen no fim não é código').toBe(false);
    });

    it('vinte mil sorteios não repetem código', () => {
        expect(new Set(codes).size).toBe(SAMPLE_SIZE);
    });

    it('nenhuma letra sai muito mais que as outras: o alfabeto inteiro aparece perto do esperado', () => {
        const counts = new Map<string, number>();

        for (const character of codes.join('')) {
            counts.set(character, (counts.get(character) ?? 0) + 1);
        }

        const expected = (SAMPLE_SIZE * RoomCode.LENGTH) / 36;

        expect(counts.size, 'o alfabeto inteiro precisa sair no sorteio').toBe(36);

        for (const [character, times] of counts) {
            expect(Math.abs(times - expected), `"${character}" saiu ${times} vezes, esperado ~${Math.round(expected)} — sorteio enviesado`).toBeLessThan(expected * 0.15);
        }
    });
});
