import { ApiClient } from './ApiClient.ts';
import type { App } from './App.ts';
import { Chat } from './Chat.ts';
import { Direct } from './Direct.ts';
import { Failure } from './Failure.ts';
import { Field } from './Field.ts';
import { Friends } from './Friends.ts';
import type {
    AuthToken,
    Channel,
    ChannelType,
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
import { Realtime, type ChannelListener, type PresenceMember } from './Realtime.ts';
import { ServerSettings } from './ServerSettings.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';
import { Voice } from './Voice.ts';

export type HubModal =
    | { type: 'server' }
    | { type: 'settings' }
    | { type: 'user' }
    | { type: 'channel'; channel: Channel | null; channelType: ChannelType };

export type RoleEditor = { role: Role | null };

export type MemberMenuPosition = { userId: number; x: number; y: number };

export type LoginMode = 'login' | 'register';

export type HomeTab = 'servers' | 'friends';

export type StageChat = 'voice' | 'text';

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
    stageChat: StageChat | null;
    railOpen: boolean;
    membersOpen: boolean;
    homeTab: HomeTab;
    inviteBanner: boolean;
    modal: HubModal | null;
    roleEditor: RoleEditor | null;
    memberMenu: MemberMenuPosition | null;
    loginMode: LoginMode;
    loginError: string;
    loginFieldErrors: Record<string, string>;
    loginBusy: boolean;
    googleWaiting: boolean;
    nicknameError: string;
    nicknameBusy: boolean;
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

type VoiceStateEvent = { channel_id: string; user_id: number; name: string; event: 'joined' | 'left' };

export class Hub {
    static readonly REFRESH_DEBOUNCE_MS = 250;
    static readonly RAIL_KEY = 'unkvoid:rail';
    static readonly MEMBERS_KEY = 'unkvoid:members';

    static readonly AVATAR_MAX_BYTES = 2 * 1024 * 1024;

    static personId(identity: string): number {
        return Number(identity.slice(identity.indexOf(':') + 1));
    }

    readonly app: App;
    readonly server: string;
    readonly api: ApiClient;
    user: User | null = null;
    config: Config | null = null;
    realtime: Realtime | null = null;
    servers: ServerSummary[] = [];
    tree: ServerTree | null = null;
    channel: Channel | null = null;
    online = new Set<number>();
    openTicket = 0;
    refreshTimer: number | null = null;
    presenceListener: ChannelListener | null = null;
    readonly voiceChannels = new Map<string, ChannelListener>();
    readonly store: Store<HubState>;
    readonly chat: Chat;
    readonly voiceChat: Chat;
    readonly voice: Voice;
    readonly settings: ServerSettings;
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
            stageChat: null,
            railOpen: localStorage.getItem(Hub.RAIL_KEY) === 'open',
            membersOpen: localStorage.getItem(Hub.MEMBERS_KEY) !== 'closed',
            homeTab: 'servers',
            inviteBanner: false,
            modal: null,
            roleEditor: null,
            memberMenu: null,
            loginMode: 'login',
            loginError: '',
            loginFieldErrors: {},
            loginBusy: false,
            googleWaiting: false,
            nicknameError: '',
            nicknameBusy: false,
        });

        this.chat = new Chat(this);
        this.voiceChat = new Chat(this);
        this.voiceChat.watched = false;
        this.voice = new Voice(app, this);
        this.voice.store.subscribe(() => this.followVoice());
        this.settings = new ServerSettings(this);
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

        const { home, stageOpen, focusedRoom, stageChat } = this.store.state;

        if (this.app.store.state.screen === 'hub') {
            const stageVisible = Boolean(this.voice.channel) && ! home && (stageOpen || focusedRoom);

            this.app.media.setStageVisible(stageVisible);
            this.voiceChat.setWatched(stageVisible && stageChat === 'voice');
        }
    }

    followVoice(): void {
        const channel = this.voice.store.state.channel;

        if (channel?.id === this.voiceChat.channel?.id) {
            return;
        }

        if (! channel) {
            this.voiceChat.close();
            this.publish({ stageChat: null });

            return;
        }

        void this.attempt(() => this.voiceChat.open(channel));
    }

    setStageChat(stageChat: StageChat | null): void {
        this.publish({ stageChat });
    }

    async attempt<Result>(work: () => Promise<Result>): Promise<Result | undefined> {
        try {
            return await work();
        } catch (failure) {
            await this.report(failure);

            return undefined;
        }
    }

    async report(failure: unknown): Promise<void> {
        const status = Failure.status(failure);

        this.app.log('hub.error', { status, message: Failure.message(failure) });

        if (status === 401) {
            this.app.toast('sua sessão expirou, entre de novo', true);
            await this.logout();

            return;
        }

        this.app.toast(status === 403 ? `sem permissão: ${Failure.message(failure)}` : Failure.message(failure), true);
    }

    channelFailed(channel: string, failure: unknown): void {
        this.app.log('realtime.channel.error', { channel, message: Failure.message(failure) });
        this.app.toast('o tempo real falhou num canal: mensagens e presença podem não chegar sozinhas', true);
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
        this.store.set({ loginMode, loginError: '', loginFieldErrors: {} });
    }

    clearLoginFieldError(field: string): void {
        if (! (field in this.store.state.loginFieldErrors)) {
            return;
        }

        const loginFieldErrors = { ...this.store.state.loginFieldErrors };

        delete loginFieldErrors[field];
        this.store.set({ loginFieldErrors });
    }

    async signIn(work: () => Promise<AuthToken>): Promise<boolean> {
        this.store.set({ loginError: '', loginFieldErrors: {}, loginBusy: true });

        try {
            const { token, user } = await work();

            this.api.setToken(token);
            this.user = user ?? await this.api.get<User>('/api/me');
            this.store.set({ loginBusy: false, googleWaiting: false });
            await this.open();

            return true;
        } catch (failure) {
            const loginFieldErrors = Field.errors(failure, ['email', 'password']);

            this.app.log('hub.login.error', { status: Failure.status(failure), message: Failure.message(failure) });
            this.store.set({
                loginError: Object.keys(loginFieldErrors).length === 0 ? Failure.message(failure) : '',
                loginFieldErrors,
                loginBusy: false,
            });

            return false;
        }
    }

    login(email: string, password: string): Promise<boolean> {
        return this.signIn(() => {
            Field.check({
                email: Field.emailProblem(email),
                password: password === '' ? 'Digite a senha.' : null,
            });

            return this.api.post<AuthToken>('/api/auth/login', { email: email.trim(), password, device: 'app' });
        });
    }

    register(email: string, password: string): Promise<boolean> {
        return this.signIn(() => {
            Field.check({
                email: Field.emailProblem(email),
                password: password.length < 8 ? 'A senha precisa ter pelo menos 8 caracteres.' : null,
            });

            return this.api.post<AuthToken>('/api/auth/register', { email: email.trim(), password, device: 'app' });
        });
    }

    async confirmNickname(name: string): Promise<void> {
        this.store.set({ nicknameError: '', nicknameBusy: true });

        try {
            Field.check({ name: Field.nicknameProblem(name) });
            this.user = await this.api.patch<User>('/api/me', { name: name.trim() });
        } catch (failure) {
            const nicknameError = Field.errors(failure, ['name']).name ?? '';

            this.app.log('hub.nickname.error', { status: Failure.status(failure), message: Failure.message(failure) });
            this.store.set({ nicknameError });

            if (Failure.status(failure) === 403) {
                await this.attempt(async () => {
                    this.user = await this.api.get<User>('/api/me');
                });
            } else if (nicknameError === '') {
                await this.report(failure);
            }
        }

        this.publish({ nicknameBusy: false });
    }

    async uploadAvatar(file: File): Promise<void> {
        if (file.size > Hub.AVATAR_MAX_BYTES) {
            this.app.toast('a foto precisa ter menos de 2 MB', true);

            return;
        }

        const updated = await this.attempt(() => this.api.upload<User>('/api/me/avatar', 'avatar', file));

        if (! updated) {
            return;
        }

        this.user = updated;
        this.publish();
    }

    async removeAvatar(): Promise<void> {
        const updated = await this.attempt(() => this.api.delete<User>('/api/me/avatar'));

        if (! updated) {
            return;
        }

        this.user = updated;
        this.publish();
    }

    clearNicknameError(): void {
        if (this.store.state.nicknameError !== '') {
            this.store.set({ nicknameError: '' });
        }
    }

    googleLogin(): Promise<boolean> {
        this.store.set({ googleWaiting: true });

        return this.signIn(async () => {
            try {
                return { token: await Tauri.invoke<string>('google_login', { server: this.server }) };
            } catch (failure) {
                this.app.log('hub.google.error', { message: Failure.message(failure) });

                throw new Error('Não deu para entrar com o Google. Tente de novo.');
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

        this.voice.watchMicErrors();
        this.voice.listenNative();
        await this.voice.applyShortcuts();

        this.config ??= await this.attempt(() => this.api.get<Config>('/api/config')) ?? null;

        if (! this.config) {
            this.store.set({ serversLoading: false });
            this.app.showOffline();

            return;
        }

        if (! this.realtime && ! await this.attempt(async () => {
            await this.connectRealtime();

            return true;
        })) {
            this.dropRealtime();
            this.store.set({ serversLoading: false });

            return;
        }

        await this.attempt(() => this.loadServers());

        await Promise.all([this.friends.load(), this.direct.loadConversations()]);
    }

    dropRealtime(): void {
        this.realtime?.disconnect();
        this.realtime = null;
    }

    async connectRealtime(): Promise<void> {
        const realtime = new Realtime((channel, failure) => this.channelFailed(channel, failure));

        this.realtime = realtime;
        realtime.on('diagnostic', detail => this.app.log(detail.event, detail.data));
        realtime.on('reconnecting', () => this.store.set({ connected: false }));
        realtime.on('reconnected', () => {
            this.store.set({ connected: true });
            void this.catchUp();
        });
        realtime.on('closed', () => {
            this.app.log('realtime.closed');
            this.app.toast('o tempo real desistiu de voltar: reabra o app para o chat e a presença voltarem', true);
            this.store.set({ connected: false });
        });

        await realtime.connect(this.app.socketUrl(), () => this.api.post<{ token: string }>('/api/sfu/session'));
        await realtime.subscribe(`user.${this.user!.id}`, {
            FriendshipUpdated: ({ friendship, removed }: { friendship: Friendship; removed: boolean }) => {
                if (removed) {
                    this.friends.store.set(state => ({ list: state.list.filter(item => item.id !== friendship.id) }));

                    return;
                }

                const known = this.friends.store.state.list.some(item => item.id === friendship.id);

                this.friends.update(friendship);

                if (! known && friendship.status === 'pending' && friendship.addressee.id === this.user?.id) {
                    this.app.toast(`${friendship.requester.name} quer ser seu amigo`);
                }
            },
            DirectMessageCreated: (payload: BroadcastDirectMessage) => {
                const { message, person, mine } = this.readDirectMessage(payload);

                this.direct.receive(message, person);

                if (! mine) {
                    this.app.sounds.message();
                }

                if (! mine && this.direct.store.state.person?.id !== person.id) {
                    this.app.toast(`${message.sender.name}: ${message.body.slice(0, 60)}`);
                }
            },
            DirectMessageUpdated: (payload: BroadcastDirectMessage) => this.direct.updateMessage(this.readDirectMessage(payload).message),
            DirectMessageDeleted: ({ id }: { id: number }) => this.direct.removeMessage(id),
            MemberRemoved: ({ server_id: serverId, reason }: { server_id: number; reason: string }) => {
                this.app.toast(reason === 'banned' ? 'você foi banido deste servidor' : 'você foi expulso deste servidor', true);

                if (this.tree?.id === serverId) {
                    void this.closeServer();
                }

                void this.attempt(() => this.loadServers());
            },
        });
    }

    async catchUp(): Promise<void> {
        this.app.log('realtime.reconnected');

        try {
            await this.loadServers();
            await Promise.all([this.friends.load(), this.direct.loadConversations(), this.direct.catchUp(), this.chat.catchUp(), this.voiceChat.catchUp()]);
        } catch (failure) {
            this.app.log('hub.catchup.error', { status: Failure.status(failure), message: Failure.message(failure) });
        }
    }

    async logout(): Promise<void> {
        await this.voice.leave();
        await this.closeServer();
        this.dropRealtime();
        this.friends.forget();
        this.direct.forget();
        this.api.setToken(null);
        this.user = null;
        this.servers = [];
        this.publish({ home: true, nicknameError: '' });

        if (this.app.store.state.screen !== 'room') {
            this.app.showEntry();
        }
    }

    async roomByCode(): Promise<void> {
        await this.voice.leave();
        this.store.set({ modal: null });
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
        Field.require(name, 'Dê um nome ao servidor.');

        const created = await this.api.post<ServerSummary>('/api/servers', { name: name.trim() });

        await this.loadServers(created.id);
        this.store.set({ inviteBanner: Boolean(this.tree?.invite_code) });

        return true;
    }

    async joinInvite(code: string): Promise<boolean> {
        Field.require(code, 'Cole o código do convite.');

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
            this.store.set({ home: false, treeLoading: true });
            await this.closeServer({ home: false, treeLoading: true });
        }

        const ticket = ++this.openTicket;
        let tree: ServerTree;

        this.store.set({ home: false, treeLoading: switching });

        try {
            tree = await this.api.get<ServerTree>(`/api/servers/${id}`);
        } catch (failure) {
            if (ticket === this.openTicket) {
                this.store.set({ treeLoading: false });
            }

            if (ticket === this.openTicket && [403, 404].includes(Failure.status(failure) ?? 0)) {
                await this.closeServer();
                await this.loadServers();
            }

            throw failure;
        }

        if (ticket !== this.openTicket) {
            return;
        }

        this.tree = tree;
        this.servers = this.servers.map(server => (server.id === tree.id ? { ...server, name: tree.name } : server));
        this.publish({ treeLoading: false });

        if (switching) {
            await this.joinPresence();
        }

        await this.subscribeVoiceStates();

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

    async closeServer(next: Partial<HubState> = { home: true, treeLoading: false }): Promise<void> {
        this.openTicket += 1;
        clearTimeout(this.refreshTimer ?? undefined);

        if (this.tree && this.presenceListener) {
            this.realtime?.unsubscribe(`server.${this.tree.id}`, this.presenceListener);
        }

        this.presenceListener = null;

        for (const [channelId, listener] of this.voiceChannels) {
            this.realtime?.unsubscribe(`channel.${channelId}`, listener);
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
        this.publish({ inviteBanner: false, stageOpen: false, focusedRoom: false, modal: null, roleEditor: null, memberMenu: null, ...next });
    }

    joinPresence(): Promise<void> {
        const channel = `server.${this.tree!.id}`;
        const listener: ChannelListener = {
            'presence.here': ({ members }: { members: PresenceMember[] }) => {
                this.online = new Set(members.map(member => Hub.personId(member.id)));
                this.publish();
            },
            'presence.joining': ({ id }: PresenceMember) => {
                this.online = new Set([...this.online, Hub.personId(id)]);
                this.publish();
            },
            'presence.leaving': ({ id }: { id: string }) => {
                this.online = new Set([...this.online].filter(known => known !== Hub.personId(id)));
                this.publish();
            },
            ServerUpdated: () => {
                clearTimeout(this.refreshTimer ?? undefined);
                this.refreshTimer = setTimeout(() => {
                    const tree = this.tree;

                    if (tree) {
                        void this.attempt(() => this.openServer(tree.id));
                    }
                }, Hub.REFRESH_DEBOUNCE_MS);
            },
        };

        this.presenceListener = listener;

        return this.realtime!.subscribe(channel, listener).catch((failure: unknown) => this.channelFailed(channel, failure));
    }

    async subscribeVoiceStates(): Promise<void> {
        const visible = new Set(this.tree!.channels.filter(channel => channel.type === 'voice').map(channel => channel.id));

        for (const [channelId, listener] of [...this.voiceChannels]) {
            if (! visible.has(channelId)) {
                this.realtime?.unsubscribe(`channel.${channelId}`, listener);
                this.voiceChannels.delete(channelId);
            }
        }

        await Promise.all([...visible].filter(channelId => ! this.voiceChannels.has(channelId)).map(channelId => {
            const listener: ChannelListener = {
                VoiceStateUpdated: ({ channel_id: id, user_id: userId, name, event }: VoiceStateEvent) => {
                    const tree = this.tree;

                    if (! tree?.channels.some(channel => channel.id === id)) {
                        return;
                    }

                    const people = (tree.voice?.[id] ?? []).filter(person => person.user_id !== userId);

                    this.tree = { ...tree, voice: { ...tree.voice, [id]: event === 'joined' ? [...people, { user_id: userId, name, sources: [] }] : people } };
                    this.publish();

                    if (this.voice.channel?.id === id) {
                        this.syncVoiceSources();
                    }
                },
            };

            this.voiceChannels.set(channelId, listener);

            return this.realtime!.subscribe(`channel.${channelId}`, listener)
                .catch((failure: unknown) => this.channelFailed(`channel.${channelId}`, failure));
        }));
    }

    syncVoiceSources(): void {
        const channel = this.voice.channel;
        const peers = this.voice.store.state.joining ? undefined : this.app.media.sfu?.peers;
        const tree = this.tree;
        const user = this.user;

        if (! tree || ! channel || ! user || ! tree.channels.some(item => item.id === channel.id)) {
            this.publish();

            return;
        }

        const muted = this.voice.muted || this.voice.serverMuted;
        const people: VoicePerson[] = peers
            ? []
            : [{ user_id: user.id, name: user.name, sources: [], muted }, ...(tree.voice?.[channel.id] ?? []).filter(person => person.user_id !== user.id)];

        for (const peer of peers?.values() ?? []) {
            if (! peer.self && ! peer.userId?.startsWith('user:')) {
                continue;
            }

            people.push({
                user_id: peer.self ? user.id : Hub.personId(peer.userId!),
                name: peer.self ? user.name : peer.name,
                sources: peer.producers.map(producer => producer.source),
                muted: peer.self ? muted : peer.producers.some(producer => producer.source === 'mic' && producer.paused),
            });
        }

        this.tree = { ...tree, voice: { ...tree.voice, [channel.id]: people } };
        this.publish();
    }

    dropFromVoice(channelId: string): void {
        const tree = this.tree;
        const people = tree?.voice?.[channelId];

        if (! tree || ! people) {
            return;
        }

        this.tree = { ...tree, voice: { ...tree.voice, [channelId]: people.filter(person => person.user_id !== this.user?.id) } };
        this.store.set({ tree: this.tree });
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

            this.publish({ home: false, stageOpen: true });
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
