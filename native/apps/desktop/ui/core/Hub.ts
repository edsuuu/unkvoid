import Echo, { type Channel as EchoChannel } from 'laravel-echo';
import Pusher from 'pusher-js';

import { ApiClient } from './ApiClient.ts';
import type { App } from './App.ts';
import { Chat } from './Chat.ts';
import { Clips } from './Clips.ts';
import { Direct } from './Direct.ts';
import { Failure } from './Failure.ts';
import { Friends } from './Friends.ts';
import type {
    AuthToken,
    Channel,
    ChannelType,
    Clip,
    Config,
    DirectMessage,
    Friendship,
    Member,
    Person,
    Role,
    ServerSummary,
    ServerTree,
    User,
    VoicePerson,
} from './Models.ts';
import { Permissions } from './Permissions.ts';
import { ServerSettings } from './ServerSettings.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';
import { Voice } from './Voice.ts';

declare global {
    interface Window {
        Pusher: typeof Pusher;
    }
}

export type HubModal =
    | { type: 'server' }
    | { type: 'settings' }
    | { type: 'user' }
    | { type: 'channel'; channel: Channel | null; channelType: ChannelType };

export type RoleEditor = { role: Role | null };

export type MemberMenuPosition = { userId: number; x: number; y: number };

export type LoginMode = 'login' | 'register';

export type HomeTab = 'servers' | 'friends';

export type HubState = {
    user: User | null;
    servers: ServerSummary[];
    serversLoading: boolean;
    serversFailed: boolean;
    tree: ServerTree | null;
    treeLoading: boolean;
    channel: Channel | null;
    online: Set<number>;
    connected: boolean;
    home: boolean;
    stageOpen: boolean;
    focusedRoom: boolean;
    railOpen: boolean;
    membersOpen: boolean;
    homeTab: HomeTab;
    inviteBanner: boolean;
    modal: HubModal | null;
    roleEditor: RoleEditor | null;
    memberMenu: MemberMenuPosition | null;
    loginMode: LoginMode;
    loginError: string;
    loginBusy: boolean;
    googleWaiting: boolean;
};

export type MemberActions = {
    nickname: boolean;
    mute: boolean;
    deafen: boolean;
    disconnect: boolean;
    kick: boolean;
    ban: boolean;
    roles: boolean;
};

export type MemberPatch = {
    nickname?: string | null;
    role_ids?: number[];
    server_mute?: boolean;
    server_deaf?: boolean;
};

type BroadcastDirectMessage = { message: Omit<DirectMessage, 'mine'>; recipient: Person };

type OnlineUser = { id: number };

type VoiceStateEvent = { channel_id: string; user_id: number; name: string; event: 'joined' | 'left' };

export class Hub {
    static readonly REFRESH_DEBOUNCE_MS = 250;
    static readonly RAIL_KEY = 'unkvoid:rail';
    static readonly MEMBERS_KEY = 'unkvoid:members';

    readonly app: App;
    readonly server: string;
    readonly api: ApiClient;
    user: User | null = null;
    config: Config | null = null;
    echo: Echo<'reverb'> | null = null;
    servers: ServerSummary[] = [];
    tree: ServerTree | null = null;
    channel: Channel | null = null;
    online = new Set<number>();
    openTicket = 0;
    refreshTimer: number | null = null;
    readonly voiceChannels = new Set<string>();
    readonly guardedSubscriptions = new WeakSet<object>();
    readonly store: Store<HubState>;
    readonly chat: Chat;
    readonly voice: Voice;
    readonly settings: ServerSettings;
    readonly clips: Clips;
    readonly friends: Friends;
    readonly direct: Direct;

    constructor(app: App, server: string) {
        this.app = app;
        this.server = server;
        this.api = new ApiClient(server);
        this.store = new Store<HubState>({
            user: null,
            servers: [],
            serversLoading: false,
            serversFailed: false,
            tree: null,
            treeLoading: false,
            channel: null,
            online: new Set(),
            connected: true,
            home: true,
            stageOpen: false,
            focusedRoom: false,
            railOpen: localStorage.getItem(Hub.RAIL_KEY) === 'open',
            membersOpen: localStorage.getItem(Hub.MEMBERS_KEY) !== 'closed',
            homeTab: 'servers',
            inviteBanner: false,
            modal: null,
            roleEditor: null,
            memberMenu: null,
            loginMode: 'login',
            loginError: '',
            loginBusy: false,
            googleWaiting: false,
        });

        this.chat = new Chat(this);
        this.voice = new Voice(app, this);
        this.settings = new ServerSettings(this);
        this.clips = new Clips(app, this);
        this.friends = new Friends(app, this);
        this.direct = new Direct(app, this);
    }

    publish(extra: Partial<HubState> = {}): void {
        this.store.set({
            user: this.user,
            servers: this.servers,
            tree: this.tree,
            channel: this.channel,
            online: this.online,
            ...extra,
        });

        const { home, stageOpen, focusedRoom } = this.store.state;

        if (this.app.store.state.screen === 'hub') {
            this.app.media.setStageVisible(Boolean(this.voice.channel) && ! home && (stageOpen || focusedRoom));
        }
    }

    async attempt<Result>(work: () => Promise<Result>): Promise<Result | undefined> {
        try {
            return await work();
        } catch (failure) {
            const status = Failure.status(failure);

            this.app.log('hub.error', { status, message: Failure.message(failure) });

            if (status === 401) {
                this.app.toast('sua sessão expirou, entre de novo', true);
                await this.logout();

                return undefined;
            }

            this.app.toast(status === 403 ? `sem permissão: ${Failure.message(failure)}` : Failure.message(failure), true);

            return undefined;
        }
    }

    listen<Payload>(subscription: EchoChannel, name: string, handler: (payload: Payload) => void): void {
        if (! this.guardedSubscriptions.has(subscription)) {
            this.guardedSubscriptions.add(subscription);
            subscription.error?.((status: unknown) => {
                this.app.log('echo.subscription.error', { status: (status as { status?: unknown } | null)?.status ?? status ?? null });
                this.app.toast('o tempo real falhou num canal: mensagens e presença podem não chegar sozinhas', true);
            });
        }

        subscription.listen(`.${name}`, handler).listen(name, handler);
    }

    async restore(): Promise<boolean> {
        if (! this.api.token) {
            return false;
        }

        try {
            this.user = await this.api.get<User>('/api/me');
        } catch (failure) {
            this.app.log('hub.restore.error', { status: Failure.status(failure), message: Failure.message(failure) });

            if (Failure.status(failure) === 401) {
                this.api.setToken(null);

                return false;
            }

            this.app.showOffline();

            return true;
        }

        await this.open();

        return true;
    }

    setLoginMode(loginMode: LoginMode): void {
        this.store.set({ loginMode, loginError: '' });
    }

    async signIn(work: () => Promise<AuthToken>): Promise<boolean> {
        this.store.set({ loginError: '', loginBusy: true });

        try {
            const { token, user } = await work();

            this.api.setToken(token);
            this.user = user ?? await this.api.get<User>('/api/me');
            this.store.set({ loginBusy: false, googleWaiting: false });
            await this.open();

            return true;
        } catch (failure) {
            this.app.log('hub.login.error', { message: Failure.message(failure) });
            this.store.set({ loginError: Failure.message(failure), loginBusy: false });

            return false;
        }
    }

    login(email: string, password: string): Promise<boolean> {
        return this.signIn(() => this.api.post<AuthToken>('/api/auth/login', { email: email.trim(), password, device: 'app' }));
    }

    register(name: string, email: string, password: string): Promise<boolean> {
        return this.signIn(() => this.api.post<AuthToken>('/api/auth/register', { name: name.trim(), email: email.trim(), password, device: 'app' }));
    }

    googleLogin(): Promise<boolean> {
        this.store.set({ googleWaiting: true });

        return this.signIn(async () => {
            try {
                return { token: await Tauri.invoke<string>('google_login', { server: this.server }) };
            } catch (failure) {
                this.app.log('hub.google.error', { message: Failure.message(failure) });

                throw new Error('o login pelo navegador não terminou, tente de novo');
            } finally {
                this.store.set({ googleWaiting: false });
            }
        });
    }

    async open(): Promise<void> {
        if (this.app.store.state.screen !== 'room') {
            this.app.store.set({ screen: 'hub' });
        }

        this.publish({ serversLoading: this.servers.length === 0 });

        this.clips.refresh();
        this.voice.watchMicErrors();
        this.voice.listenShortcuts();
        await this.voice.applyShortcuts();

        this.config ??= await this.attempt(() => this.api.get<Config>('/api/config')) ?? null;

        if (! this.config) {
            this.store.set({ serversLoading: false });
            this.app.showOffline();

            return;
        }

        if (! this.echo && ! await this.attempt(async () => {
            this.connectEcho();

            return true;
        })) {
            (this.echo as Echo<'reverb'> | null)?.disconnect();
            this.echo = null;
            this.store.set({ serversLoading: false });

            return;
        }

        await this.attempt(() => this.loadServers());

        await Promise.all([this.friends.load(), this.direct.loadConversations()]);
    }

    connectEcho(): void {
        const { host, port, key, scheme } = this.config!.reverb;

        window.Pusher = Pusher;
        this.echo = new Echo({
            broadcaster: 'reverb',
            Pusher,
            key,
            wsHost: host,
            wsPort: port,
            wssPort: port,
            forceTLS: scheme === 'https',
            enabledTransports: ['ws', 'wss'],
            authEndpoint: `${this.server}/broadcasting/auth`,
            auth: { headers: { Authorization: `Bearer ${this.api.token}` } },
        });

        this.echo.connector.pusher.connection.bind('state_change', ({ current }: { current: string }) => {
            this.store.set({ connected: current === 'connected' });
        });

        const own = this.echo.private(`user.${this.user!.id}`);

        this.listen<{ clip: Clip }>(own, 'ClipUpdated', ({ clip }) => this.clips.update(clip));
        this.listen<{ friendship: Friendship; removed: boolean }>(own, 'FriendshipUpdated', ({ friendship, removed }) => {
            if (removed) {
                this.friends.store.set(state => ({ list: state.list.filter(item => item.id !== friendship.id) }));

                return;
            }

            const known = this.friends.store.state.list.some(item => item.id === friendship.id);

            this.friends.update(friendship);

            if (! known && friendship.status === 'pending' && friendship.addressee.id === this.user?.id) {
                this.app.toast(`${friendship.requester.name} quer ser seu amigo`);
            }
        });
        this.listen<BroadcastDirectMessage>(own, 'DirectMessageCreated', payload => {
            const { message, person, mine } = this.readDirectMessage(payload);

            this.direct.receive(message, person);

            if (! mine) {
                this.app.sounds.message();
            }

            if (! mine && this.direct.store.state.person?.id !== person.id) {
                this.app.toast(`${message.sender.name}: ${message.body.slice(0, 60)}`);
            }
        });
        this.listen<BroadcastDirectMessage>(own, 'DirectMessageUpdated', payload => this.direct.updateMessage(this.readDirectMessage(payload).message));
        this.listen<{ id: number }>(own, 'DirectMessageDeleted', ({ id }) => this.direct.removeMessage(id));
        this.listen<{ server_id: number; reason: string }>(own, 'MemberRemoved', ({ server_id: serverId, reason }) => {
            this.app.toast(reason === 'banned' ? 'você foi banido deste servidor' : 'você foi expulso deste servidor', true);

            if (this.tree?.id === serverId) {
                void this.closeServer();
            }

            void this.attempt(() => this.loadServers());
        });
    }

    async logout(): Promise<void> {
        await this.voice.leave();
        await this.closeServer();
        this.echo?.disconnect();
        this.echo = null;
        this.clips.forget();
        this.friends.forget();
        this.direct.forget();
        this.api.setToken(null);
        this.user = null;
        this.servers = [];
        this.publish({ home: true });

        if (this.app.store.state.screen !== 'room') {
            this.app.showEntry();
        }
    }

    async roomByCode(): Promise<void> {
        await this.voice.leave();
        this.store.set({ modal: null });
        this.app.setTab('broadcast');
        this.app.showEntry();
    }

    async loadServers(openId: number | null = null): Promise<void> {
        this.store.set({ serversLoading: true, serversFailed: false });

        try {
            this.servers = await this.api.get<ServerSummary[]>('/api/servers');
        } catch (failure) {
            this.store.set({ serversFailed: true });

            throw failure;
        } finally {
            this.store.set({ serversLoading: false });
        }

        this.publish();

        const target = openId ?? this.tree?.id;

        if (target && this.servers.some(server => server.id === target)) {
            await this.openServer(target);

            return;
        }

        await this.closeServer();
    }

    async createServer(name: string): Promise<boolean> {
        const created = await this.api.post<ServerSummary>('/api/servers', { name: name.trim() });

        await this.loadServers(created.id);
        this.store.set({ inviteBanner: Boolean(this.tree?.invite_code) });

        return true;
    }

    async joinInvite(code: string): Promise<boolean> {
        const joined = await this.api.post<ServerSummary>(`/api/invites/${code.trim()}`);

        await this.loadServers(joined.id);

        return true;
    }

    async openDirect(person: Person): Promise<void> {
        this.publish({ home: true, memberMenu: null });

        await this.direct.open(person);
    }

    showHome(): void {
        this.publish({ home: true, memberMenu: null });
    }

    async openServer(id: number): Promise<void> {
        const switching = this.tree?.id !== id;

        if (switching) {
            await this.closeServer();
        }

        const ticket = ++this.openTicket;
        let tree: ServerTree;

        this.store.set({ home: false, treeLoading: switching });

        try {
            tree = await this.api.get<ServerTree>(`/api/servers/${id}`);
        } catch (failure) {
            if (ticket === this.openTicket && [403, 404].includes(Failure.status(failure) ?? 0)) {
                await this.closeServer();
                await this.loadServers();
            }

            throw failure;
        } finally {
            if (ticket === this.openTicket) {
                this.store.set({ treeLoading: false });
            }
        }

        if (ticket !== this.openTicket) {
            return;
        }

        this.tree = tree;
        this.servers = this.servers.map(server => (server.id === tree.id ? { ...server, name: tree.name } : server));

        if (switching) {
            this.joinPresence();
        }

        this.subscribeVoiceStates();

        const voiceChannel = this.voice.channel;

        if (voiceChannel) {
            this.voice.channel = tree.channels.find(channel => channel.id === voiceChannel.id) ?? voiceChannel;
            this.voice.publish();
        }

        this.syncVoiceSources();

        const current = tree.channels.find(channel => channel.id === this.channel?.id);

        if (current) {
            this.channel = current;
            this.publish();

            return;
        }

        if (this.channel || switching) {
            await this.openChannel(tree.channels.find(channel => channel.type === 'text') ?? null);
        }
    }

    async closeServer(): Promise<void> {
        this.openTicket += 1;
        clearTimeout(this.refreshTimer ?? undefined);

        if (this.tree) {
            this.echo?.leave(`server.${this.tree.id}`);
        }

        for (const channelId of this.voiceChannels) {
            this.echo?.leave(`channel.${channelId}`);
        }

        this.voiceChannels.clear();

        const voiceChannel = this.voice.channel;

        if (voiceChannel && this.tree?.channels.some(channel => channel.id === voiceChannel.id)) {
            await this.voice.leave();
        }

        this.chat.close();
        this.tree = null;
        this.channel = null;
        this.online = new Set();
        this.publish({ inviteBanner: false, home: true, treeLoading: false, stageOpen: false, focusedRoom: false, modal: null, roleEditor: null, memberMenu: null });
    }

    joinPresence(): void {
        const presence = this.echo!.join(`server.${this.tree!.id}`);

        presence
            .here((users: OnlineUser[]) => {
                this.online = new Set(users.map(user => user.id));
                this.publish();
            })
            .joining((user: OnlineUser) => {
                this.online = new Set([...this.online, user.id]);
                this.publish();
            })
            .leaving((user: OnlineUser) => {
                this.online = new Set([...this.online].filter(id => id !== user.id));
                this.publish();
            });

        this.listen(presence, 'ServerUpdated', () => {
            clearTimeout(this.refreshTimer ?? undefined);
            this.refreshTimer = setTimeout(() => {
                const tree = this.tree;

                if (tree) {
                    void this.attempt(() => this.openServer(tree.id));
                }
            }, Hub.REFRESH_DEBOUNCE_MS);
        });
    }

    subscribeVoiceStates(): void {
        const visible = new Set(this.tree!.channels.filter(channel => channel.type === 'voice').map(channel => channel.id));

        for (const channelId of [...this.voiceChannels]) {
            if (! visible.has(channelId)) {
                this.echo!.leave(`channel.${channelId}`);
                this.voiceChannels.delete(channelId);
            }
        }

        for (const channelId of visible) {
            if (this.voiceChannels.has(channelId)) {
                continue;
            }

            this.voiceChannels.add(channelId);
            this.listen<VoiceStateEvent>(this.echo!.private(`channel.${channelId}`), 'VoiceStateUpdated', ({ channel_id: id, user_id: userId, name, event }) => {
                const tree = this.tree;

                if (! tree?.channels.some(channel => channel.id === id)) {
                    return;
                }

                const people = (tree.voice?.[id] ?? []).filter(person => person.user_id !== userId);

                this.tree = { ...tree, voice: { ...tree.voice, [id]: event === 'joined' ? [...people, { user_id: userId, name, sources: [] }] : people } };
                this.publish();

                if (this.voice.channel?.id === id && this.app.media.sfu) {
                    this.syncVoiceSources();
                }
            });
        }
    }

    syncVoiceSources(): void {
        this.voice.closeEmptyClipList();

        const channel = this.voice.channel;
        const peers = this.app.media.sfu?.peers;
        const tree = this.tree;

        if (! tree || ! channel || ! peers || ! tree.channels.some(item => item.id === channel.id)) {
            this.publish();

            return;
        }

        const people: VoicePerson[] = [];

        for (const peer of peers.values()) {
            if (! peer.self && ! peer.userId?.startsWith('user:')) {
                continue;
            }

            people.push({
                user_id: peer.self ? this.user!.id : Number(peer.userId!.slice('user:'.length)),
                name: peer.self ? this.user!.name : peer.name,
                sources: peer.producers.map(producer => producer.source),
                muted: peer.self ? this.voice.muted || this.voice.serverMuted : peer.producers.some(producer => producer.source === 'mic' && producer.paused),
            });
        }

        this.tree = { ...tree, voice: { ...tree.voice, [channel.id]: people } };
        this.publish();
    }

    me(): Member | null {
        return this.tree?.members.find(member => member.user_id === this.user?.id) ?? null;
    }

    can(flag: number): boolean {
        return this.tree !== null && Permissions.has(this.tree.me.permissions, flag);
    }

    async openChannel(channel: Channel | null): Promise<void> {
        if (channel?.type === 'voice') {
            if (this.voice.channel?.id !== channel.id && ! Permissions.has(channel.permissions, Permissions.CONNECT)) {
                this.app.toast('você não pode entrar neste canal de voz', true);

                return;
            }

            const focusedRoom = this.store.state.focusedRoom;

            if (this.voice.channel && this.voice.channel.id !== channel.id) {
                await this.voice.leave();
            }

            this.publish({ home: false, stageOpen: true, focusedRoom });
            await this.voice.join(channel);

            return;
        }

        this.channel = channel;
        this.publish({ home: false, stageOpen: false });

        if (! channel) {
            this.chat.close();

            return;
        }

        await this.chat.open(channel);
    }

    showStage(stageOpen: boolean): void {
        this.publish({ stageOpen, focusedRoom: false });
    }

    setFocusedRoom(focusedRoom: boolean): void {
        this.publish({ focusedRoom, stageOpen: true, memberMenu: null });
    }

    showHomeTab(homeTab: HomeTab): void {
        this.direct.close();
        this.store.set({ homeTab });
    }

    toggleMembers(): void {
        const membersOpen = ! this.store.state.membersOpen;

        localStorage.setItem(Hub.MEMBERS_KEY, membersOpen ? 'open' : 'closed');
        this.store.set({ membersOpen });
    }

    toggleRail(): void {
        const railOpen = ! this.store.state.railOpen;

        localStorage.setItem(Hub.RAIL_KEY, railOpen ? 'open' : 'closed');
        this.store.set({ railOpen });
    }

    openModal(modal: HubModal): void {
        this.store.set({ modal, memberMenu: null });
    }

    closeModal(): void {
        this.store.set({ modal: null });
    }

    closeInviteBanner(): void {
        this.store.set({ inviteBanner: false });
    }

    copyInvite(): Promise<boolean> {
        return this.app.copy(this.tree?.invite_code ?? '', 'convite copiado');
    }

    closeTopmost(): boolean {
        const { memberMenu, roleEditor, modal } = this.store.state;

        if (memberMenu) {
            this.store.set({ memberMenu: null });

            return true;
        }

        if (roleEditor) {
            this.store.set({ roleEditor: null });

            return true;
        }

        if (modal) {
            this.store.set({ modal: null });

            return true;
        }

        return false;
    }

    readDirectMessage({ message, recipient }: BroadcastDirectMessage): { message: DirectMessage; person: Person; mine: boolean } {
        const mine = message.sender.id === this.user?.id;

        return { message: { ...message, mine }, person: mine ? recipient : message.sender, mine };
    }

    openMemberMenu(member: Member, x: number, y: number): void {
        this.store.set({ memberMenu: { userId: member.user_id, x, y } });
    }

    closeMemberMenu(): void {
        this.store.set({ memberMenu: null });
    }

    memberActions(member: Member): MemberActions {
        const self = member.user_id === this.user?.id;
        const below = this.tree !== null && Permissions.outranks(this.tree, this.me(), member) && ! self;
        const inVoice = Object.values(this.tree?.voice ?? {}).some(people => people.some(person => person.user_id === member.user_id));

        return {
            nickname: self || (below && this.can(Permissions.MANAGE_SERVER)),
            mute: below && this.can(Permissions.MUTE_MEMBERS),
            deafen: below && this.can(Permissions.DEAFEN_MEMBERS),
            disconnect: below && inVoice && this.can(Permissions.MOVE_MEMBERS),
            kick: below && this.can(Permissions.KICK_MEMBERS),
            ban: below && this.can(Permissions.BAN_MEMBERS),
            roles: below && this.can(Permissions.MANAGE_ROLES),
        };
    }

    assignableRoles(): Role[] {
        const tree = this.tree;

        return (tree?.roles ?? []).filter(role => ! role.is_everyone && role.position < tree!.me.top_position);
    }

    async updateMember(member: Member, body: MemberPatch): Promise<void> {
        const done = await this.attempt(async () => {
            await this.api.patch(`/api/servers/${this.tree!.id}/members/${member.user_id}`, body);

            return true;
        });

        if (done && ! ('role_ids' in body)) {
            this.closeMemberMenu();
        }
    }

    async disconnectMember(member: Member): Promise<void> {
        const voice = this.tree!.voice ?? {};
        const channelId = Object.keys(voice).find(id => voice[id].some(person => person.user_id === member.user_id));

        const done = await this.attempt(async () => {
            await this.api.delete(`/api/channels/${channelId}/voice/members/${member.user_id}`);

            return true;
        });

        if (done) {
            this.closeMemberMenu();
        }
    }

    async kickMember(member: Member): Promise<void> {
        if (! await this.app.confirm(`Expulsar ${member.name} do servidor?`, 'Expulsar')) {
            return;
        }

        const done = await this.attempt(async () => {
            await this.api.delete(`/api/servers/${this.tree!.id}/members/${member.user_id}`);

            return true;
        });

        if (done) {
            this.closeMemberMenu();
        }
    }

    async banMember(member: Member, reason: string): Promise<void> {
        if (! await this.app.confirm(`Banir ${member.name}? A pessoa não consegue voltar até ser perdoada.`, 'Banir')) {
            return;
        }

        const done = await this.attempt(async () => {
            await this.api.post(`/api/servers/${this.tree!.id}/bans/${member.user_id}`, { reason: reason.trim() || undefined });

            return true;
        });

        if (done) {
            this.closeMemberMenu();
        }
    }
}
