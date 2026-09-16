import { beforeAll, describe, expect, it } from 'vitest';

import { App } from '../../ui/core/App.ts';
import type { Hub } from '../../ui/core/Hub.ts';
import { Permissions } from '../../ui/core/Permissions.ts';
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
    it('os bits batem com a tabela do SERVIDORES.md', () => {
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
        hub.echo = { private: () => quiet, join: () => quiet, leave() {}, disconnect() {} };

        responses.set('GET /api/servers', () => servers);
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

    it('entrar na voz liga o mic mutado, e mutado antes de publicar', async () => {
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

    it('com "Silenciar ao entrar" desligado, a pessoa entra falando; a regra continua sendo o padrão', async () => {
        expect(voice.store.state.preferences.muteOnJoin, 'o padrão é entrar mutado').toBe(true);

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

    it('com a sala por código aberta, entrar ou sair da conta não esconde a sala', async () => {
        expect(app.store.state.screen).toBe('room');

        await hub.open();
        expect(app.store.state.screen, 'entrar na conta não troca a tela da sala').toBe('room');

        await hub.logout();
        expect(app.store.state.screen, 'sair da conta também não').toBe('room');
    });
});
