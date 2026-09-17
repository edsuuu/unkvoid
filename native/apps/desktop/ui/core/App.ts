import { Failure } from './Failure.ts';
import { Hub } from './Hub.ts';
import { Media } from './Media.ts';
import type { Channel } from './Models.ts';
import { Platform } from './Platform.ts';
import { RoomCode } from './RoomCode.ts';
import { SfuClient, type RoomIdentity } from './SfuClient.ts';
import { Sharing } from './Sharing.ts';
import { Sounds } from './Sounds.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';

export type AppScreen = 'update' | 'offline' | 'entry' | 'room' | 'hub';

export type AppTab = 'broadcast' | 'clips';

export type Toast = { id: number; message: string; error: boolean };

export type Dialog = { message: string; confirmLabel: string; resolve: (accepted: boolean) => void };

export type AppState = {
    screen: AppScreen;
    tab: AppTab;
    updateStatus: string;
    updateProgress: number | null;
    offlineTitle: string;
    offlineStatus: string;
    entryName: string;
    entryCode: string;
    entryError: string;
    room: string | null;
    roomError: string | null;
    toasts: Toast[];
    dialog: Dialog | null;
    logsOpen: boolean;
    logsText: string;
    logPath: string;
    fatal: string | null;
};

export class App {
    static readonly UPDATE_EVERY_MS = 6 * 60 * 60 * 1000;
    static readonly INSTALL_KEY = 'unkvoid.instalacao';
    static readonly TOAST_MS = 6000;
    static readonly TOAST_INFO_MS = 3500;
    static readonly MAX_LOG_ENTRIES = 250;
    static readonly MAX_LOG_CHARS = 64 * 1024;
    static readonly NAME_KEY = 'unkvoid:name';
    static readonly ROOM_KEY = 'unkvoid:last-room';
    static readonly RECENT_ROOMS_KEY = 'unkvoid:recent-rooms';
    static readonly MAX_RECENT_ROOMS = 5;
    static readonly WEBRTC_RELOAD_KEY = 'unkvoid:webrtc-reload';
    static readonly SERVER: string = import.meta.env?.VITE_SERVER ?? localStorage.getItem('server') ?? 'https://unkvoid.com';

    logs: string[] = [];
    logChars = 0;
    reconnectAttempt = 0;
    reconnectTimer: number | null = null;
    rejoin: { channel: Channel | null; room: string | null } | null = null;
    toastCounter = 0;
    name = '';
    readonly store: Store<AppState>;
    readonly sounds: Sounds;
    readonly media: Media;
    readonly sharing: Sharing;
    readonly hub: Hub;

    static installId(): string {
        let id = localStorage.getItem(App.INSTALL_KEY);

        if (! id) {
            id = crypto.randomUUID();
            localStorage.setItem(App.INSTALL_KEY, id);
        }

        return id;
    }

    constructor() {
        this.store = new Store<AppState>({
            screen: 'update',
            tab: 'broadcast',
            updateStatus: 'Procurando atualizações…',
            updateProgress: null,
            offlineTitle: '',
            offlineStatus: '',
            entryName: '',
            entryCode: '',
            entryError: '',
            room: null,
            roomError: null,
            toasts: [],
            dialog: null,
            logsOpen: false,
            logsText: '',
            logPath: '',
            fatal: null,
        });

        this.log('app.start', {
            userAgent: navigator.userAgent,
            platform: navigator.platform,
            hasWebRTC: typeof RTCPeerConnection !== 'undefined',
        });

        this.sounds = new Sounds();
        this.media = new Media(this);
        this.sharing = new Sharing(this);
        this.hub = new Hub(this, App.SERVER);
    }

    socketUrl(): string {
        return this.hub.config?.sfu ?? `${App.SERVER.replace(/^http/, 'ws')}/sfu`;
    }

    async start(): Promise<void> {
        this.checkWebRTC();
        void Tauri.invoke('stop_broadcast').catch(() => null);
        this.sharing.loadPreferences();

        window.addEventListener('offline', () => void this.dropConnection());
        window.addEventListener('online', () => {
            if (this.store.state.screen === 'offline') {
                this.reconnectAttempt = 0;
                this.showOffline();
            }
        });
        document.addEventListener('keydown', event => this.onKeyDown(event));
        document.addEventListener('mousemove', () => this.media.wakeUp());
        document.addEventListener('visibilitychange', () => this.media.visibilityChanged());

        this.showDownloadProgress();

        if (await this.serverAnswered()) {
            await this.update();
        }

        await this.expandWindow();
        setInterval(() => void this.update(), App.UPDATE_EVERY_MS);

        if (! await this.serverAnswered()) {
            return;
        }

        await this.enter();
    }

    async enter(): Promise<void> {
        this.store.set({ updateStatus: 'Entrando…', updateProgress: null });

        if (await this.hub.restore()) {
            await this.resumeAfterOffline();

            return;
        }

        this.showEntry();
        await this.resumeAfterOffline();
    }

    async dropConnection(): Promise<void> {
        if (! ['hub', 'room', 'entry'].includes(this.store.state.screen)) {
            this.paintOffline();

            return;
        }

        const room = this.store.state.room;

        this.rejoin = { channel: this.hub.voice.channel, room };
        this.log('connection.lost', { channel: this.rejoin.channel?.id ?? null, room });
        this.store.set({ screen: 'offline', room: null, roomError: null });
        this.paintOffline();

        await this.hub.voice.leave();
        await this.media.tearDown();

        this.showOffline();
    }

    async resumeAfterOffline(): Promise<void> {
        const rejoin = this.rejoin;

        if (this.store.state.screen === 'offline') {
            return;
        }

        this.rejoin = null;

        if (rejoin?.room) {
            await this.openRoom(rejoin.room);
        }

        if (rejoin?.channel && this.hub.user) {
            await this.hub.attempt(() => this.hub.openChannel(rejoin.channel));
        }
    }

    checkWebRTC(): void {
        if (typeof RTCPeerConnection !== 'undefined') {
            return;
        }

        if (! sessionStorage.getItem(App.WEBRTC_RELOAD_KEY)) {
            sessionStorage.setItem(App.WEBRTC_RELOAD_KEY, '1');
            this.log('webrtc.reload');
            location.reload();

            return;
        }

        this.log('webrtc.missing', { userAgent: navigator.userAgent });
    }

    showDownloadProgress(): void {
        void Tauri.listen<[number, number]>('update:progress', ({ payload: [downloaded, total] }) => {
            if (! total) {
                this.store.set({ updateStatus: `Baixando a atualização… ${(downloaded / 1024 / 1024).toFixed(1)} MB`, updateProgress: null });

                return;
            }

            const percent = Math.min(100, Math.round((downloaded / total) * 100));

            this.store.set({ updateStatus: `Baixando a atualização… ${percent}%`, updateProgress: percent });
        })?.catch?.((failure: unknown) => this.log('update.listen.error', { message: Failure.message(failure) }));
    }

    async expandWindow(): Promise<void> {
        try {
            await Tauri.invoke('expand_window');
        } catch (failure) {
            this.log('window.expand.error', { message: Failure.message(failure) });
        }
    }

    async update(): Promise<void> {
        if (Platform.isLinux()) {
            return;
        }

        if (this.media.sfu) {
            return;
        }

        try {
            const version = await Tauri.invoke<string | null>('check_update');

            if (! version) {
                return;
            }

            this.store.set({ updateStatus: `Instalando a versão ${version}…` });
            await Tauri.invoke('restart');
        } catch (failure) {
            this.log('update.error', { message: Failure.message(failure) });
        }
    }

    async serverAnswered(): Promise<boolean> {
        if (await this.reachable()) {
            return true;
        }

        this.showOffline();

        return false;
    }

    async reachable(): Promise<boolean> {
        try {
            const response = await fetch(`${App.SERVER}/health`);

            if (! response.ok) {
                throw new Error(`o servidor respondeu ${response.status}`);
            }

            return true;
        } catch (failure) {
            this.log('server.health.error', { message: Failure.message(failure) });

            return false;
        }
    }

    showOffline(): void {
        this.reconnectAttempt += 1;
        this.store.set({ screen: 'offline' });
        this.paintOffline();

        clearTimeout(this.reconnectTimer ?? undefined);
        this.reconnectTimer = setTimeout(async () => {
            if (await this.serverAnswered()) {
                this.reconnectAttempt = 0;
                await this.enter();
            }
        }, Math.min(2000 * this.reconnectAttempt, 10000));
    }

    paintOffline(): void {
        const withoutNetwork = ! navigator.onLine;

        this.store.set({
            offlineTitle: withoutNetwork ? 'Sem internet' : 'Servidor sem resposta',
            offlineStatus: withoutNetwork
                ? `Este computador está sem rede. Tentando de novo… (tentativa ${this.reconnectAttempt})`
                : `A sua internet está funcionando. Tentando de novo… (tentativa ${this.reconnectAttempt})`,
        });
    }

    onKeyDown(event: KeyboardEvent): void {
        if (event.key !== 'Escape' || event.defaultPrevented) {
            return;
        }

        if (this.store.state.dialog) {
            this.resolveDialog(false);

            return;
        }

        if (this.hub.closeTopmost()) {
            return;
        }

        if (this.store.state.logsOpen) {
            this.closeLogs();

            return;
        }

        if (this.sharing.store.state.open) {
            this.sharing.close();

            return;
        }

        if (this.media.store.state.fullscreen) {
            void this.media.toggleFullscreen(this.media.store.state.fullscreen);
        }
    }

    confirm(message: string, confirmLabel = 'Confirmar'): Promise<boolean> {
        return new Promise<boolean>(resolve => {
            this.store.state.dialog?.resolve(false);
            this.store.set({ dialog: { message, confirmLabel, resolve } });
        });
    }

    resolveDialog(accepted: boolean): void {
        const dialog = this.store.state.dialog;

        if (! dialog) {
            return;
        }

        this.store.set({ dialog: null });
        dialog.resolve(accepted);
    }

    setTab(tab: AppTab): void {
        if (tab === 'clips' && ! Platform.isWindows()) {
            return;
        }

        this.store.set({ tab });
        this.media.paintWatching();

        if (tab === 'clips') {
            void this.hub.clips.load();

            return;
        }

        this.hub.clips.closePlayer();
    }

    showEntry(): void {
        const account = this.hub.store.state.user;

        this.store.set({
            screen: 'entry',
            entryName: account?.name ?? localStorage.getItem(App.NAME_KEY) ?? '',
            entryCode: localStorage.getItem(App.ROOM_KEY) ?? '',
            entryError: '',
        });
    }

    setEntry(patch: Partial<AppState>): void {
        this.store.set(patch);
    }

    createRoom(): Promise<void> {
        const label = this.store.state.entryCode.trim().toLowerCase();

        return this.openRoom(label || RoomCode.generate());
    }

    joinRoom(): Promise<void> {
        return this.openRoom(this.store.state.entryCode.trim().toLowerCase());
    }

    async openRoom(code: string): Promise<void> {
        const name = (this.hub.store.state.user?.name ?? this.store.state.entryName).trim();

        this.store.set({ entryError: '' });

        if (name === '') {
            this.store.set({ entryError: 'Escolha um nome primeiro.' });

            return;
        }

        if (! RoomCode.isValid(code)) {
            this.store.set({ entryError: 'Use 3–32 caracteres: letras, números e hífens (sem hífen no começo ou fim).' });

            return;
        }

        localStorage.setItem(App.NAME_KEY, name);
        localStorage.setItem(App.ROOM_KEY, code);
        localStorage.setItem(App.RECENT_ROOMS_KEY, JSON.stringify([code, ...this.recentRooms().filter(recent => recent !== code)].slice(0, App.MAX_RECENT_ROOMS)));

        this.name = name;
        await this.hub.voice.leave();
        await this.connect(code);
    }

    recentRooms(): string[] {
        try {
            const saved: unknown = JSON.parse(localStorage.getItem(App.RECENT_ROOMS_KEY) ?? '[]');

            return Array.isArray(saved) ? saved.filter((code): code is string => typeof code === 'string' && RoomCode.isValid(code)) : [];
        } catch {
            return [];
        }
    }

    async roomIdentity(code: string): Promise<RoomIdentity> {
        if (! this.hub.user) {
            return { room: code, name: this.name, installId: App.installId() };
        }

        return { token: (await this.hub.api.post<{ token: string }>(`/api/rooms/${code}/token`)).token };
    }

    async connect(code: string): Promise<void> {
        const sfu = new SfuClient();

        this.store.set({ screen: 'room', room: code, roomError: null });
        this.media.setStageVisible(true);

        try {
            await this.media.enterRoom(sfu, () => this.roomIdentity(code));

            if (this.media.sfu === sfu && sfu.canWatch() && ! sfu.videoCodecs.some(codec => /h264/i.test(codec))) {
                this.log('device.h264.missing', {
                    codecs: sfu.videoCodecs,
                    receiver: globalThis.RTCRtpReceiver?.getCapabilities?.('video')?.codecs?.map(codec => codec.mimeType) ?? null,
                    sender: globalThis.RTCRtpSender?.getCapabilities?.('video')?.codecs?.map(codec => codec.mimeType) ?? null,
                    userAgent: navigator.userAgent,
                });
                this.fail('o motor da janela desta máquina não recebe H.264 pelo WebRTC. Dá para transmitir, mas não para assistir. Abra Logs e mande a linha device.h264.missing.');
            }
        } catch (failure) {
            const message = `não deu para entrar na sala: ${Failure.message(failure)}`;

            this.log('room.enter.error', { message });

            if (this.media.sfu !== sfu) {
                return;
            }

            await this.leave();

            this.store.set({ entryError: message });
        }
    }

    async leave(): Promise<void> {
        this.log('room.leave');
        this.store.set({ room: null, roomError: null });
        this.showEntry();

        await this.media.tearDown().catch((failure: unknown) => this.log('room.leave.error', { message: Failure.message(failure) }));
    }

    fail(message: string): void {
        this.log('ui.error', { message });

        if (this.store.state.screen !== 'room') {
            this.toast(message, true);

            return;
        }

        this.store.set({ roomError: message });
    }

    dismissRoomError(): void {
        this.store.set({ roomError: null });
    }

    toast(message: string, error = false): void {
        this.log('room.toast', { message, error });

        const id = ++this.toastCounter;

        this.store.set(state => ({ toasts: [...state.toasts, { id, message, error }] }));
        setTimeout(() => this.dismissToast(id), error ? App.TOAST_MS : App.TOAST_INFO_MS);
    }

    dismissToast(id: number): void {
        this.store.set(state => ({ toasts: state.toasts.filter(toast => toast.id !== id) }));
    }

    async copy(text: string, message: string): Promise<boolean> {
        try {
            await navigator.clipboard.writeText(text);
            this.toast(message);

            return true;
        } catch (failure) {
            this.log('clipboard.error', { message: Failure.message(failure) });
            this.toast('não deu para copiar — selecione o texto à mão.', true);

            return false;
        }
    }

    log(event: string, data: unknown = {}): void {
        const line = `${new Date().toISOString()} ${event} ${JSON.stringify(data)}`;

        if (Tauri.available()) {
            Tauri.invoke('log_line', { line })?.catch?.(() => null);
        }

        this.logs.push(line);
        this.logChars += line.length + (this.logs.length > 1 ? 1 : 0);

        while (this.logs.length > App.MAX_LOG_ENTRIES || this.logChars > App.MAX_LOG_CHARS) {
            const removed = this.logs.shift();

            this.logChars -= (removed?.length ?? 0) + 1;
        }

        if (this.store?.state.logsOpen) {
            this.store.set({ logsText: this.logs.join('\n') });
        }
    }

    async openLogs(): Promise<void> {
        this.store.set({ logsOpen: true, logsText: this.logs.join('\n') });

        const path = await Tauri.invoke<string>('log_path').catch((failure: unknown) => {
            this.log('logs.path.error', { message: Failure.message(failure) });

            return '';
        });

        this.store.set({ logPath: path ?? '' });
    }

    closeLogs(): void {
        this.store.set({ logsOpen: false });
    }

    clearLogs(): void {
        this.logs = [];
        this.logChars = 0;
        this.store.set({ logsText: '' });
    }

    async copyLogs(): Promise<boolean> {
        try {
            await navigator.clipboard.writeText(this.logs.join('\n'));

            return true;
        } catch (failure) {
            this.log('logs.copy.error', { message: Failure.message(failure) });
            this.toast('não deu para copiar os logs — selecione o texto à mão.', true);

            return false;
        }
    }

    reportFatal(error: unknown): void {
        this.log('ui.fatal', { message: Failure.message(error), stack: (error as Error | null)?.stack ?? null });
        this.store.set({ fatal: Failure.message(error) });
    }
}
