import type { MediaKind } from 'mediasoup-client/types';

import type { App } from './App.ts';
import { Broadcast } from './Broadcast.ts';
import { Failure } from './Failure.ts';
import { Platform } from './Platform.ts';
import type {
    IdentitySource,
    JoinResponse,
    PlainConsumerResponse,
    ProducerInfo,
    Reconnected,
    SfuClient,
    SfuPeer,
    SourceName,
    StatsReport,
} from './SfuClient.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';

export type TileKind = 'screen' | 'camera';

export type Tile = {
    key: string;
    kind: TileKind;
    peerId: string;
    name: string;
    stream: MediaStream | null;
    native: { port: number; producerId: string } | null;
    self: boolean;
};

export type PeerView = SfuPeer & { latency: number | null; missing: boolean };

export type ImageProperty = 'brightness' | 'contrast' | 'saturation' | 'blur';

export type ImageSettings = Record<ImageProperty, number>;

export type AudioState = { volume: number; muted: boolean };

export type TileStats =
    | { paused: true }
    | {
        paused: false;
        height: number | null;
        fps: number;
        ping: number | null;
        rate: number | null;
        loss: number | null;
        buffer: number;
        totalLost: number | null;
        jitter: number | null;
    };

export type MediaState = {
    connecting: boolean;
    connectedAt: number | null;
    reconnecting: boolean;
    tiles: Tile[];
    focused: string | null;
    fullscreen: string | null;
    idle: boolean;
    peers: PeerView[];
    pending: boolean;
    ping: number | null;
    paused: string[];
    audio: Record<string, AudioState>;
    voices: Record<string, AudioState>;
    nativeMuted: Record<string, boolean>;
    image: Record<TileKind, ImageSettings>;
    selfView: boolean;
    watchers: Record<string, string[]>;
};

type PublishedPeer = { peerId: string; producers?: ProducerInfo[] };

type ProducerRef = { producerId: string; peerId: string; kind?: MediaKind; source?: SourceName };

export class Media {
    static readonly IDLE_MS = 3000;

    static readonly MAX_SCREENS = (navigator.hardwareConcurrency ?? 4) <= 4 ? 2 : 4;

    static readonly IMAGE_KEYS: Record<ImageProperty, string> = { brightness: 'unkvoid.brilho', contrast: 'unkvoid.contraste', saturation: 'unkvoid.saturacao', blur: 'unkvoid.desfoque' };

    static readonly VOICES_KEY = 'unkvoid:voice-volumes';

    static readonly FULL_VOICE: AudioState = { volume: 100, muted: false };

    static readonly IMAGE_LIMITS: Record<ImageProperty, [number, number, number]> = {
        brightness: [50, 250, 100],
        contrast: [50, 250, 100],
        saturation: [50, 250, 100],
        blur: [0, 20, 0],
    };

    readonly app: App;
    sfu: SfuClient | null = null;
    broadcast: Broadcast | null = null;
    readonly remoteAudios = new Map<string, HTMLAudioElement>();
    readonly micAudios = new Map<string, HTMLAudioElement>();
    deafened = false;
    readonly nativeWatching = new Map<string, string>();
    readonly consumingProducers = new Set<string>();
    readonly consumerSources = new Map<string, SourceName | undefined>();
    readonly hiddenPeers = new Set<string>();
    readonly videos = new Map<string, HTMLVideoElement>();
    readonly mediaStatsTimers = new Map<string, number>();
    peopleStatsTimer: number | null = null;
    awayTimer: number | null = null;
    idleTimer: number | null = null;
    warnedNoWebRTC = false;
    tearingDown: Promise<void> = Promise.resolve();
    readonly kickedPeers = new Set<string>();
    stageVisible = true;
    readonly store: Store<MediaState>;
    readonly stats: Store<Record<string, TileStats>>;

    static cameraKey(peerId: string): string {
        return `${peerId}/camera`;
    }

    static imageKey(kind: TileKind, property: ImageProperty): string {
        return kind === 'camera' ? `${Media.IMAGE_KEYS[property]}.camera` : Media.IMAGE_KEYS[property];
    }

    static canPickOutput(): boolean {
        return typeof HTMLMediaElement !== 'undefined' && 'setSinkId' in HTMLMediaElement.prototype;
    }

    static loadImage(kind: TileKind): ImageSettings {
        const image = {} as ImageSettings;

        for (const property of Object.keys(Media.IMAGE_KEYS) as ImageProperty[]) {
            const [least, most, fallback] = Media.IMAGE_LIMITS[property];
            const saved = Number(localStorage.getItem(Media.imageKey(kind, property)));

            image[property] = saved >= least && saved <= most ? saved : fallback;
        }

        return image;
    }

    constructor(app: App) {
        this.app = app;
        this.store = new Store<MediaState>(this.initialState());
        this.stats = new Store<Record<string, TileStats>>({});
    }

    loadVoices(): Record<string, AudioState> {
        const voices: Record<string, AudioState> = {};

        try {
            const saved = JSON.parse(localStorage.getItem(Media.VOICES_KEY) ?? '{}') as Record<string, Partial<AudioState> | null> | null;

            for (const [userId, voice] of Object.entries(saved ?? {})) {
                const volume = Number(voice?.volume);

                if (volume >= 0 && volume <= 100) {
                    voices[userId] = { volume, muted: Boolean(voice?.muted) };
                }
            }
        } catch (failure) {
            this.app.log('media.voice.volumes.error', { message: Failure.message(failure) });
        }

        return voices;
    }

    initialState(): MediaState {
        return {
            connecting: false,
            connectedAt: null,
            reconnecting: false,
            tiles: [],
            watchers: {},
            focused: null,
            fullscreen: null,
            idle: false,
            peers: [],
            pending: false,
            ping: null,
            paused: [],
            audio: {},
            voices: this.loadVoices(),
            nativeMuted: {},
            image: { screen: Media.loadImage('screen'), camera: Media.loadImage('camera') },
            selfView: false,
        };
    }

    tile(key: string): Tile | null {
        return this.store.state.tiles.find(tile => tile.key === key) ?? null;
    }

    isPaused(peerId: string): boolean {
        return this.store.state.paused.includes(peerId);
    }

    async enterRoom(sfu: SfuClient, identity: IdentitySource, alongside: (joined: JoinResponse) => unknown = () => null): Promise<JoinResponse> {
        await this.tearingDown;
        this.attachSfu(sfu);
        this.store.set({ connecting: true });

        try {
            const joined = await sfu.connect(this.app.socketUrl(), identity);

            if (this.sfu !== sfu) {
                return joined;
            }

            await Promise.all([this.consumePeers(joined.peers), alongside(joined)]);

            this.store.set({ connectedAt: Date.now() });
            this.refreshPeople();
            clearInterval(this.peopleStatsTimer ?? undefined);
            this.peopleStatsTimer = setInterval(() => void this.refreshPeopleStats().catch((failure: unknown) => this.app.log('media.people.error', { message: Failure.message(failure) })), 2000);

            return joined;
        } finally {
            if (this.sfu === sfu) {
                this.store.set({ connecting: false });
            }
        }
    }

    consumePeers(peers: PublishedPeer[] | undefined, cap: number = Media.MAX_SCREENS): Promise<unknown[]> {
        let screens = this.store.state.tiles.filter(tile => tile.kind === 'screen').length;

        return Promise.all((peers ?? []).flatMap(peer => (peer.producers ?? []).map(producer =>
            producer.source === 'screen' && screens++ >= cap
                ? this.app.log('media.consume.capped', { peerId: peer.peerId, cap })
                : this.consume({ ...producer, peerId: peer.peerId }))));
    }

    attachSfu(sfu: SfuClient): void {
        this.sfu = sfu;
        this.broadcast = new Broadcast(sfu);
        sfu.on('diagnostic', detail => this.app.log(detail.event, detail.data));
        sfu.on('reconnecting', detail => {
            this.app.log('sfu.reconnecting', detail);
            this.store.set({ reconnecting: true });

            if (detail.attempt === 1) {
                void this.dropIfOffline(sfu);
            }
        });
        sfu.on('reconnected', detail => void this.afterReconnect(detail));
        sfu.on('closed', () => {
            if (this.sfu !== sfu) {
                return;
            }

            this.store.set({ reconnecting: false });
            this.app.fail('a conexão caiu e não voltou. Saia e entre de novo.');

            if (this.app.hub.voice.channel) {
                void this.app.hub.voice.leave();
            }
        });
        sfu.on('newProducer', detail => {
            if (detail.source === 'screen') {
                this.app.sounds.streamStarted();
                this.app.toast(`${sfu.peers.get(detail.peerId)?.name ?? 'alguém'} começou a transmitir`);
            }

            void this.consumePeers([{ peerId: detail.peerId, producers: [detail] }]);
        });
        sfu.on('watchers', detail => this.rememberWatchers(detail.producerId, detail.watchers ?? []));
        sfu.on('peersChanged', () => {
            this.refreshPeople();
            this.app.hub.syncVoiceSources();
        });
        sfu.on('peerKicked', detail => {
            this.kickedPeers.add(detail.peerId);
            this.app.toast(`${detail.name} foi removido`);
        });
        sfu.on('kicked', detail => {
            if (this.sfu !== sfu) {
                return;
            }

            this.app.fail(detail?.reason ?? 'você foi removido');

            if (this.app.hub.voice.channel) {
                void this.app.hub.voice.leave();
            }
        });
        sfu.on('peerJoined', detail => {
            this.app.sounds.joined();
            this.app.toast(`${detail.name} entrou`);
        });
        sfu.on('peerLeft', detail => {
            if (! this.kickedPeers.delete(detail.peerId) && detail.name) {
                this.app.toast(`${detail.name} saiu`);
            }

            this.forgetPeer(detail.peerId);
        });
        sfu.on('replaced', () => void this.replaced(sfu));
        sfu.on('producerDead', detail => void this.app.sharing.died(detail));
        sfu.on('producerClosed', detail => {
            if (detail.source === 'screen') {
                this.app.sounds.streamStopped();
            }

            this.forgetProducer(detail);
        });
        sfu.on('consumerClosed', detail => {
            this.consumerSources.delete(detail?.consumerId);
            this.forgetProducer(detail);
        });
    }

    async replaced(sfu: SfuClient): Promise<void> {
        if (this.sfu !== sfu) {
            return;
        }

        if (this.app.store.state.room) {
            await this.app.leave();
        }

        this.app.fail('sua conta entrou nesta chamada por outro dispositivo, e esta conexão foi encerrada.');
    }

    async dropIfOffline(sfu: SfuClient): Promise<void> {
        if (this.sfu !== sfu || await this.app.reachable()) {
            return;
        }

        if (this.sfu === sfu && this.store.state.reconnecting) {
            await this.app.dropConnection();
        }
    }

    forgetPeer(peerId: string): void {
        this.showScreen(peerId, null);
        this.showScreen(Media.cameraKey(peerId), null);
        this.stopNativeTile(`${peerId}/mic`);

        for (const [producerId, audio] of [...this.micAudios]) {
            if (audio.dataset.remote === peerId) {
                this.forgetProducer({ producerId, peerId, kind: 'audio', source: 'mic' });
            }
        }
    }

    forgetProducer({ producerId, peerId, kind, source }: ProducerRef): void {
        if (source === 'mic') {
            const audio = this.micAudios.get(producerId);

            (audio?.srcObject as MediaStream | null | undefined)?.getTracks?.().forEach(track => track.stop());
            audio?.remove();
            this.micAudios.delete(producerId);
            void this.stopNative(producerId);

            return;
        }

        if (source === 'camera') {
            this.showScreen(Media.cameraKey(peerId), null);

            return;
        }

        if (kind === 'video') {
            this.showScreen(peerId, null);

            return;
        }

        if (source === 'screenAudio') {
            this.dropScreenAudio(peerId);
        }

        void this.stopNative(producerId);
    }

    dropScreenAudio(peerId: string): void {
        const audio = this.remoteAudios.get(peerId);

        (audio?.srcObject as MediaStream | null | undefined)?.getTracks?.().forEach(track => track.stop());
        audio?.remove();
        this.remoteAudios.delete(peerId);
        this.syncAudio(peerId);
    }

    refreshPeople(): void {
        const tiles = this.store.state.tiles;
        const sharingNow = this.app.sharing.store.state.active;
        const peers: PeerView[] = [...(this.sfu?.peers?.values() ?? [])].map(peer => ({
            ...peer,
            sharing: peer.self ? sharingNow : peer.sharing,
            latency: this.sfu?.peerLatency?.get(peer.peerId) ?? null,
            missing: ! peer.self && Boolean(peer.sharing) && ! tiles.some(tile => tile.key === peer.peerId),
        }));

        this.store.set({
            peers,
            pending: peers.some(peer => peer.missing),
            ping: this.sfu?.transportRttMs ?? this.sfu?.lastRttMs ?? null,
        });
    }

    async refreshPeopleStats(): Promise<void> {
        await this.sfu?.updatePeerLatency?.();
        this.refreshPeople();
    }

    async afterReconnect({ resumed, peers }: Reconnected): Promise<void> {
        this.app.log('sfu.reconnected', { resumed, peers: peers?.length ?? 0 });
        this.store.set({ reconnecting: false });

        if (resumed) {
            return;
        }

        if (this.app.sharing.store.state.active) {
            try {
                await this.broadcast!.republish();
                this.app.log('broadcast.republished', { producerId: this.broadcast!.videoProducerId });
            } catch (failure) {
                this.app.log('broadcast.republish.error', { message: Failure.message(failure) });
                await this.app.sharing.stop();
                this.app.fail(`a transmissão caiu com o servidor e não voltou: ${Failure.message(failure)}`);
            }
        }

        for (const tile of this.store.state.tiles) {
            this.showScreen(tile.key, null);
        }

        this.store.set({ selfView: false });

        this.nativeWatching.clear();
        await Tauri.invoke('stop_watch', { producerId: null }).catch((failure: unknown) => this.app.log('media.native.stop.error', { message: Failure.message(failure) }));

        await this.consumePeers(peers);
        this.refreshPeople();
    }

    rememberWatchers(producerId: string, watchers: { peerId: string; name: string }[]): void {
        const key = this.tileKeyOf(producerId);

        if (! key) {
            return;
        }

        const others = watchers.filter(watcher => watcher.peerId !== this.sfu?.peerId).map(watcher => watcher.name);

        this.store.set(state => ({ watchers: { ...state.watchers, [key]: others } }));
    }

    tileKeyOf(producerId: string): string | null {
        if (this.broadcast?.videoProducerId === producerId) {
            return this.sfu?.peerId ?? null;
        }

        for (const peer of this.sfu?.peers?.values() ?? []) {
            const producer = peer.producers.find(item => item.producerId === producerId);

            if (producer) {
                return producer.source === 'camera' ? Media.cameraKey(peer.peerId) : peer.peerId;
            }
        }

        return null;
    }

    async removeStoppedPeer(peerId: string): Promise<void> {
        try {
            await this.sfu!.request('removePeer', { peerId });
            this.app.log('peer.removed', { peerId });
        } catch (failure) {
            this.app.log('peer.remove.error', { peerId, message: Failure.message(failure) });
            this.app.fail(`não foi possível remover: ${Failure.message(failure)}`);
        }
    }

    async watchPeer(peerId: string): Promise<void> {
        const peer = this.sfu?.peers?.get(peerId);

        await this.consumePeers(peer ? [peer] : [], Infinity);
        this.refreshPeople();
    }

    async refreshWatch(): Promise<void> {
        for (const peerId of [...(this.sfu?.peers?.keys() ?? [])]) {
            await this.watchPeer(peerId);
        }

        this.refreshPeople();
    }

    async toggleSelfView(): Promise<void> {
        const sfu = this.sfu;
        const peerId = sfu?.peerId;
        const producerId = this.broadcast?.videoProducerId;

        if (! sfu || ! peerId || ! producerId) {
            return;
        }

        try {
            if (this.tile(peerId)) {
                await Promise.all(sfu.consumersOf(peerId).map(consumerId => sfu.closeConsumer(consumerId)));
                this.showScreen(peerId, null);
                this.store.set({ selfView: false });

                return;
            }

            await this.consume({ producerId, peerId, kind: 'video', source: 'screen' });
            this.store.set({ selfView: true });
        } catch (failure) {
            this.app.log('media.self.error', { message: Failure.message(failure) });
            this.app.fail(`não deu para ver a própria transmissão: ${Failure.message(failure)}`);
        }
    }

    async togglePause(peerId: string): Promise<void> {
        const paused = ! this.isPaused(peerId);
        const audio = this.remoteAudios.get(peerId);

        try {
            await this.sfu!.setPeerPaused(peerId, paused, 'video');
        } catch (failure) {
            this.app.log('media.pause.error', { peerId, message: Failure.message(failure) });
            this.app.fail(`não deu para ${paused ? 'pausar' : 'retomar'}: ${Failure.message(failure)}`);

            return;
        }

        if (paused) {
            audio?.pause();
        } else {
            void audio?.play().catch((failure: unknown) => this.app.log('media.resume.error', { peerId, message: Failure.message(failure) }));
        }

        this.store.set(state => ({
            paused: paused ? [...state.paused, peerId] : state.paused.filter(item => item !== peerId),
        }));
        this.app.log('media.paused', { peerId, paused });
    }

    setStageVisible(visible: boolean): void {
        if (this.stageVisible === visible) {
            return;
        }

        this.stageVisible = visible;
        this.paintWatching();
    }

    visibilityChanged(): void {
        clearTimeout(this.awayTimer ?? undefined);
        this.awayTimer = setTimeout(() => this.paintWatching(), document.hidden ? 2000 : 0);
    }

    paintWatching(): void {
        const { tiles, fullscreen } = this.store.state;
        const away = document.hidden || ! this.stageVisible;

        for (const tile of tiles) {
            const peerId = tile.key;
            const paused = away || (Boolean(fullscreen) && fullscreen !== tile.key);
            const screen = this.sfu?.peers?.get(peerId)?.producers?.find(item => item.source === 'screen');

            if (tile.kind !== 'screen' || this.isPaused(peerId) || paused === this.hiddenPeers.has(peerId)) {
                continue;
            }

            this.hiddenPeers[paused ? 'add' : 'delete'](peerId);
            void (this.nativeWatching.has(screen?.producerId ?? '')
                ? Tauri.invoke('watch_mute', { producerId: screen?.producerId, muted: paused })
                : this.sfu?.setPeerPaused(peerId, paused, 'video'))?.catch((failure: unknown) => {
                this.hiddenPeers[paused ? 'delete' : 'add'](peerId);
                this.app.log('media.hidden', { peerId, paused, message: Failure.message(failure) });
            });
        }
    }

    async consume({ producerId, peerId: ownerPeerId, kind, source }: ProducerRef): Promise<void> {
        if (this.sfu?.canWatch?.() === false && Platform.isLinux()) {
            return this.consumeNative({ producerId, peerId: ownerPeerId, kind, source });
        }

        if (this.consumingProducers.has(producerId) || this.sfu?.consumersHasProducer?.(producerId)) {
            return;
        }

        this.consumingProducers.add(producerId);
        this.app.log('media.consume.start', { producerId });

        try {
            const response = await this.sfu!.consume(producerId);
            const { consumer, peerId } = response;
            const origin: SourceName | undefined = response.source ?? source;

            this.consumerSources.set(consumer.id, origin);

            if (origin === 'mic') {
                this.playMic(producerId, peerId, consumer.track);

                return;
            }

            if (origin === 'camera') {
                this.showScreen(Media.cameraKey(peerId), new MediaStream([consumer.track]), 'camera');

                return;
            }

            if (consumer.kind === 'audio') {
                this.playScreenAudio(peerId, consumer.track);

                return;
            }

            this.showScreen(peerId, new MediaStream([consumer.track]));
            this.app.log('media.consume.ready', { producerId, peerId, kind: consumer.kind });
        } catch (failure) {
            this.app.log('media.consume.error', { producerId, peerId: ownerPeerId, message: Failure.message(failure) });

            if (this.sfu?.canWatch?.() === false && ! this.warnedNoWebRTC) {
                this.warnedNoWebRTC = true;
                this.app.fail('este sistema não tem WebRTC no motor da janela: dá para transmitir, mas ainda não dá para assistir.');
            } else if (this.sfu?.canWatch?.() !== false && kind !== 'audio') {
                this.app.fail(`não deu para assistir: ${Failure.message(failure)}`);
            }
        } finally {
            this.consumingProducers.delete(producerId);
        }
    }

    playScreenAudio(peerId: string, track: MediaStreamTrack): void {
        const audio = document.createElement('audio');

        audio.srcObject = new MediaStream([track]);
        audio.autoplay = true;
        audio.volume = 0;
        audio.muted = true;
        audio.onplay = () => this.app.log('media.audio.playing', { peerId });
        audio.onerror = () => this.app.log('media.audio.error', {
            peerId,
            message: audio.error?.message ?? `media error ${audio.error?.code ?? 'unknown'}`,
        });
        audio.dataset.remote = peerId;
        document.body.appendChild(audio);
        this.remoteAudios.set(peerId, audio);
        this.syncAudio(peerId);
        void this.routeAudio([audio]);
        void audio.play().catch((failure: unknown) => this.app.log('media.audio.autoplay.error', { peerId, message: Failure.message(failure) }));
    }

    playMic(producerId: string, peerId: string, track: MediaStreamTrack): void {
        const audio = document.createElement('audio');

        audio.srcObject = new MediaStream([track]);
        audio.autoplay = true;
        audio.dataset.remote = peerId;
        audio.dataset.source = 'mic';
        audio.dataset.user = this.sfu?.peers?.get(peerId)?.userId ?? '';
        this.applyVoice(audio);
        document.body.appendChild(audio);
        this.micAudios.set(producerId, audio);
        void this.routeAudio([audio]);
        void audio.play().catch((failure: unknown) => this.app.log('media.mic.autoplay.error', { peerId, message: Failure.message(failure) }));
    }

    syncAudio(peerId: string): void {
        const audio = this.remoteAudios.get(peerId);

        this.store.set(state => {
            const next = { ...state.audio };

            if (audio) {
                next[peerId] = { volume: Math.round(audio.volume * 100), muted: audio.muted };
            } else {
                delete next[peerId];
            }

            return { audio: next };
        });
    }

    setVolume(peerId: string, value: number): void {
        const audio = this.remoteAudios.get(peerId);

        if (! audio) {
            return;
        }

        audio.volume = value / 100;
        audio.muted = value === 0;
        this.syncAudio(peerId);
        this.app.log('media.audio.volume', { peerId, volume: value / 100, muted: audio.muted });
    }

    toggleAudioMute(peerId: string): void {
        const audio = this.remoteAudios.get(peerId);

        if (! audio) {
            return;
        }

        audio.muted = ! audio.muted;

        if (! audio.muted && audio.volume === 0) {
            audio.volume = 1;
        }

        this.syncAudio(peerId);
        this.app.log('media.audio.mute', { peerId, muted: audio.muted });
    }

    canAdjustVoices(): boolean {
        return ! (Platform.isLinux() && this.sfu?.canWatch?.() === false);
    }

    voiceOf(userId: string): AudioState {
        return this.store.state.voices[userId] ?? Media.FULL_VOICE;
    }

    applyVoice(audio: HTMLAudioElement): void {
        const voice = this.voiceOf(audio.dataset.user ?? '');

        audio.volume = voice.volume / 100;
        audio.muted = this.deafened || voice.muted;
    }

    saveVoice(userId: string, voice: AudioState): void {
        if (userId === '') {
            return;
        }

        const voices = { ...this.store.state.voices, [userId]: voice };

        if (voice.volume === Media.FULL_VOICE.volume && ! voice.muted) {
            delete voices[userId];
        }

        localStorage.setItem(Media.VOICES_KEY, JSON.stringify(voices));
        this.store.set({ voices });

        for (const audio of this.micAudios.values()) {
            if (audio.dataset.user === userId) {
                this.applyVoice(audio);
            }
        }

        this.app.log('media.voice.volume', { userId, ...voice });
    }

    setVoiceVolume(userId: string, value: number): void {
        this.saveVoice(userId, { volume: value, muted: value === 0 });
    }

    toggleVoiceMute(userId: string): void {
        const voice = this.voiceOf(userId);
        const muted = ! voice.muted;

        this.saveVoice(userId, { volume: ! muted && voice.volume === 0 ? Media.FULL_VOICE.volume : voice.volume, muted });
    }

    async routeAudio(audios: HTMLAudioElement[]): Promise<void> {
        const deviceId = this.app.hub.voice.store.state.preferences.speaker;

        if (! Media.canPickOutput()) {
            return;
        }

        try {
            await Promise.all(audios.filter(audio => audio.sinkId !== deviceId).map(audio => audio.setSinkId(deviceId)));
        } catch (failure) {
            this.app.log('media.output.error', { deviceId, message: Failure.message(failure) });

            if (deviceId === '') {
                this.app.toast(`não deu para tocar na saída de áudio padrão: ${Failure.message(failure)}`, true);

                return;
            }

            this.app.toast('a saída de áudio escolhida não respondeu: o som voltou para a saída padrão', true);
            await this.app.hub.voice.setPreference('speaker', '');
        }
    }

    async applyOutput(): Promise<void> {
        this.app.sounds.setOutput(this.app.hub.voice.store.state.preferences.speaker);
        await this.routeAudio([...this.remoteAudios.values(), ...this.micAudios.values()]);
    }

    async outputsChanged(): Promise<void> {
        const chosen = this.app.hub.voice.store.state.preferences.speaker;

        if (chosen === '' || ! Media.canPickOutput()) {
            return;
        }

        try {
            const outputs = (await navigator.mediaDevices.enumerateDevices()).filter(device => device.kind === 'audiooutput' && device.deviceId !== '');

            if (outputs.length === 0 || outputs.some(device => device.deviceId === chosen)) {
                return;
            }
        } catch (failure) {
            this.app.log('media.output.list.error', { message: Failure.message(failure) });

            return;
        }

        this.app.log('media.output.gone', { deviceId: chosen });
        this.app.toast('a saída de áudio escolhida sumiu: o som voltou para a saída padrão', true);
        await this.app.hub.voice.setPreference('speaker', '');
    }

    async setDeafened(deafened: boolean): Promise<void> {
        this.deafened = deafened;

        for (const audio of this.micAudios.values()) {
            this.applyVoice(audio);
        }

        if (deafened) {
            for (const [peerId, audio] of this.remoteAudios) {
                audio.muted = true;
                this.syncAudio(peerId);
            }
        }

        for (const [producerId, key] of this.nativeWatching) {
            if (key.endsWith('/mic')) {
                await Tauri.invoke('watch_mute', { producerId, muted: deafened })
                    .catch((failure: unknown) => this.app.log('voice.deafen.error', { producerId, message: Failure.message(failure) }));
            }
        }

        const sfu = this.sfu;

        if (! sfu) {
            return;
        }

        for (const [consumerId, peerId] of sfu.consumerPeers) {
            if (sfu.consumers.get(consumerId)?.kind === 'audio' && ! this.isPaused(peerId)) {
                await sfu.tolerate(deafened ? 'pauseConsumer' : 'resumeConsumer', { consumerId });
            }
        }
    }

    async consumeNative({ producerId, peerId, kind, source }: ProducerRef): Promise<void> {
        if (this.nativeWatching.has(producerId)) {
            return;
        }

        this.nativeWatching.set(producerId, peerId);
        this.app.log('media.native.start', { producerId, peerId, source });

        let consumerId: string | null = null;

        try {
            const keyBase64 = await Tauri.invoke<string>('watch_key');
            const consumer = await this.sfu!.request<PlainConsumerResponse>('consumePlain', {
                producerId,
                srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64 },
            });

            consumerId = consumer.consumerId;

            const origin = consumer.source ?? source;
            const media = consumer.kind ?? kind ?? 'video';
            const tileKey = origin === 'camera' ? Media.cameraKey(peerId) : origin === 'mic' ? `${peerId}/mic` : peerId;

            this.nativeWatching.set(producerId, tileKey);

            const port = await Tauri.invoke<number>('watch_native', {
                consumer: {
                    producerId,
                    kind: media,
                    address: `${consumer.ip}:${consumer.port}`,
                    serverKey: consumer.srtpParameters.keyBase64,
                    payloadType: consumer.payloadType,
                    ssrc: consumer.ssrc ?? null,
                    rtx: consumer.rtx ?? null,
                },
            });

            await this.sfu!.request('resumeConsumer', { consumerId: consumer.consumerId });

            if (media === 'video') {
                this.showNativeTile(tileKey, peerId, producerId, consumer.name, port, origin);
            } else {
                await Tauri.invoke('watch_mute', { producerId, muted: origin === 'screenAudio' || this.deafened });
            }

            this.app.log('media.native.ready', { producerId, peerId, source: origin });
        } catch (failure) {
            this.nativeWatching.delete(producerId);
            this.app.log('media.native.error', { producerId, peerId, consumerId, message: Failure.message(failure) });

            if (consumerId) {
                await this.sfu?.closeConsumer(consumerId).catch((problem: unknown) => this.app.log('media.native.cleanup.error', { producerId, message: Failure.message(problem) }));
            }

            await Tauri.invoke('stop_watch', { producerId }).catch((problem: unknown) => this.app.log('media.native.stop.error', { producerId, message: Failure.message(problem) }));
            this.app.fail(`não deu para assistir: ${Failure.message(failure)}`);
        }
    }

    async stopNative(producerId: string): Promise<void> {
        if (! this.nativeWatching.has(producerId)) {
            return;
        }

        this.nativeWatching.delete(producerId);
        await Tauri.invoke('stop_watch', { producerId }).catch((failure: unknown) => this.app.log('media.native.stop.error', { producerId, message: Failure.message(failure) }));
    }

    stopNativeTile(tileKey: string): void {
        for (const [producerId, key] of [...this.nativeWatching]) {
            if (key === tileKey) {
                void this.stopNative(producerId);
            }
        }
    }

    showNativeTile(tileKey: string, peerId: string, producerId: string, name: string | null, port: number, source: SourceName | undefined): void {
        const camera = source === 'camera';

        this.upsertTile({
            key: tileKey,
            kind: camera ? 'camera' : 'screen',
            peerId,
            name: (name ?? 'alguém') + (camera ? ' (câmera)' : ''),
            stream: null,
            native: { port, producerId },
            self: false,
        });

        if (! camera) {
            this.store.set(state => ({ nativeMuted: { ...state.nativeMuted, [tileKey]: true } }));
        }

        this.refreshPeople();
        this.paintWatching();
    }

    toggleNativeMute(tileKey: string): void {
        const tile = this.tile(tileKey);

        if (! tile) {
            return;
        }

        const muted = ! this.store.state.nativeMuted[tileKey];
        const audio = this.sfu?.peers?.get(tile.peerId)?.producers?.find(item => item.source === 'screenAudio');

        this.store.set(state => ({ nativeMuted: { ...state.nativeMuted, [tileKey]: muted } }));

        if (audio) {
            void Tauri.invoke('watch_mute', { producerId: audio.producerId, muted })
                .catch((failure: unknown) => this.app.log('media.native.mute.error', { peerId: tile.peerId, message: Failure.message(failure) }));
        }
    }

    upsertTile(tile: Tile): void {
        this.store.set(state => {
            const index = state.tiles.findIndex(item => item.key === tile.key);
            const tiles = [...state.tiles];

            if (index === -1) {
                tiles.push(tile);
            } else {
                tiles[index] = tile;
            }

            return { tiles };
        });
    }

    showScreen(key: string, stream: MediaStream | null, kind: TileKind = 'screen'): void {
        if (stream) {
            const camera = kind === 'camera';
            const peerId = camera ? key.split('/')[0] : key;
            const owner = this.sfu?.peers?.get(peerId);
            const name = camera
                ? `${owner?.self ? 'você' : owner?.name ?? 'alguém'} (câmera)`
                : owner?.self ? `${owner.name} (você, sem som)` : owner?.name ?? 'transmitindo';

            this.upsertTile({ key, kind, peerId, name, stream, native: null, self: Boolean(owner?.self) });

            if (! camera) {
                this.startMediaStats(key);
            }

            this.refreshPeople();
            this.paintWatching();

            return;
        }

        const tile = this.tile(key);

        this.stopNativeTile(key);
        tile?.stream?.getTracks?.().forEach(track => track.stop());

        if (this.remoteAudios.has(key)) {
            this.dropScreenAudio(key);
        }

        clearInterval(this.mediaStatsTimers.get(key));
        this.mediaStatsTimers.delete(key);
        this.hiddenPeers.delete(key);
        this.videos.delete(key);

        if (! tile) {
            return;
        }

        const stats = { ...this.stats.state };

        delete stats[key];
        this.stats.replace(stats);

        this.store.set(state => {
            const nativeMuted = { ...state.nativeMuted };
            const watchers = { ...state.watchers };

            delete nativeMuted[key];
            delete watchers[key];

            return {
                tiles: state.tiles.filter(item => item.key !== key),
                watchers,
                focused: state.focused === key ? null : state.focused,
                nativeMuted,
            };
        });

        if (this.store.state.fullscreen === key) {
            void this.toggleFullscreen(key);
        }

        this.refreshPeople();
    }

    async closeTile(key: string): Promise<void> {
        const tile = this.tile(key);

        if (! tile || (tile.self && tile.kind === 'camera')) {
            return;
        }

        if (! tile.native) {
            const sources: (SourceName | undefined)[] = tile.kind === 'camera' ? ['camera'] : ['screen', 'screenAudio'];
            const consumerIds = [...this.consumerSources]
                .filter(([consumerId, source]) => this.sfu?.consumerPeers?.get(consumerId) === tile.peerId && sources.includes(source))
                .map(([consumerId]) => consumerId);

            await Promise.all(consumerIds.map(consumerId => this.sfu!.closeConsumer(consumerId)));

            for (const consumerId of consumerIds) {
                this.consumerSources.delete(consumerId);
            }
        }

        this.showScreen(key, null);

        if (tile.self) {
            this.store.set({ selfView: false });
        }
    }

    registerVideo(key: string, video: HTMLVideoElement): void {
        this.videos.set(key, video);
    }

    unregisterVideo(key: string, video: HTMLVideoElement): void {
        if (this.videos.get(key) === video) {
            this.videos.delete(key);
        }
    }

    setImage(kind: TileKind, property: ImageProperty, value: number): void {
        localStorage.setItem(Media.imageKey(kind, property), String(value));
        this.store.set(state => ({ image: { ...state.image, [kind]: { ...state.image[kind], [property]: value } } }));
    }

    resetImage(kind: TileKind): void {
        for (const property of Object.keys(Media.IMAGE_KEYS) as ImageProperty[]) {
            localStorage.removeItem(Media.imageKey(kind, property));
        }

        this.store.set(state => ({ image: { ...state.image, [kind]: Media.loadImage(kind) } }));
    }

    async inboundReport(peerId: string): Promise<StatsReport | null> {
        const sfu = this.sfu;

        if (! sfu?.recvTransport) {
            return null;
        }

        const consumer = sfu.consumersOf(peerId)
            .map(consumerId => sfu.consumers.get(consumerId))
            .find(candidate => candidate?.kind === 'video');

        if (! consumer) {
            return null;
        }

        const ssrc = consumer.rtpParameters?.encodings?.[0]?.ssrc;
        const reports = await sfu.stats();

        return reports.find(report => report.type === 'inbound-rtp' && report.ssrc === ssrc) ?? null;
    }

    startMediaStats(key: string): void {
        clearInterval(this.mediaStatsTimers.get(key));

        let runs = 0;
        let lastFrames = 0;
        let lastInbound: StatsReport | null = null;
        let lastSample = performance.now();

        const refresh = async (): Promise<void> => {
            if (! this.tile(key)) {
                clearInterval(this.mediaStatsTimers.get(key));
                this.mediaStatsTimers.delete(key);

                return;
            }

            if (this.isPaused(key)) {
                this.stats.set({ [key]: { paused: true } });

                return;
            }

            const inbound = await this.inboundReport(key).catch((failure: unknown) => {
                this.app.log('media.stats.error', { peerId: key, message: Failure.message(failure) });

                return null;
            });
            const video = this.videos.get(key);
            const now = performance.now();
            const elapsedMs = Math.max(now - lastSample, 1);
            const seconds = elapsedMs / 1000;
            const buffer = video?.buffered?.length
                ? Math.max(0, video.buffered.end(video.buffered.length - 1) - video.currentTime)
                : 0;

            const quality = video?.getVideoPlaybackQuality?.();
            const frames = quality?.totalVideoFrames ?? 0;
            const fps = Math.max(0, Math.round((frames - lastFrames) * 1000 / elapsedMs));
            const ping = this.sfu?.transportRttMs ?? this.sfu?.lastRttMs ?? null;
            const since = inbound && lastInbound ? lastInbound : null;

            const lost = inbound && since ? Math.max(0, inbound.packetsLost - since.packetsLost) : 0;
            const received = inbound && since ? Math.max(0, inbound.packetsReceived - since.packetsReceived) : 0;
            const loss = since && lost + received ? lost * 100 / (lost + received) : since ? 0 : null;
            const rate = inbound && since ? (inbound.bytesReceived - since.bytesReceived) * 8 / seconds / 1e6 : null;
            const finiteRate = rate !== null && Number.isFinite(rate) ? rate : null;
            const finiteLoss = loss !== null && Number.isFinite(loss) ? loss : null;

            this.stats.set({
                [key]: {
                    paused: false,
                    height: video?.videoHeight || null,
                    fps,
                    ping,
                    rate: finiteRate,
                    loss: finiteLoss,
                    buffer,
                    totalLost: inbound?.packetsLost ?? null,
                    jitter: inbound?.jitter == null ? null : Math.round(inbound.jitter * 1000),
                },
            });

            if (++runs % 5 === 0) {
                this.app.log('media.stats', {
                    peerId: key,
                    pingMs: ping,
                    bufferSeconds: Number(buffer.toFixed(2)),
                    fps,
                    mbps: finiteRate === null ? null : Number(finiteRate.toFixed(2)),
                    lossPercent: finiteLoss === null ? null : Number(finiteLoss.toFixed(2)),
                    packetsLost: inbound?.packetsLost ?? null,
                    jitterMs: inbound?.jitter == null ? null : Math.round(inbound.jitter * 1000),
                    framesDropped: quality?.droppedVideoFrames ?? null,
                    framesDecoded: frames,
                });
            }

            lastFrames = frames;
            lastInbound = inbound;
            lastSample = now;
        };

        this.mediaStatsTimers.set(key, setInterval(() => void refresh(), 1000));
        void refresh();
    }

    focus(key: string): void {
        this.store.set(state => ({ focused: state.focused === key ? null : key }));
    }

    async toggleFullscreen(key: string): Promise<void> {
        const fullscreen = this.store.state.fullscreen === key ? null : key;

        this.store.set({ fullscreen });
        this.app.log('media.fullscreen', { peerId: key, on: Boolean(fullscreen) });
        this.paintWatching();
        this.wakeUp();

        try {
            await Tauri.setFullscreen(Boolean(fullscreen));
        } catch (failure) {
            this.app.log('media.fullscreen.error', { peerId: key, message: Failure.message(failure) });
        }
    }

    wakeUp(): void {
        clearTimeout(this.idleTimer ?? undefined);
        this.idleTimer = null;

        if (this.store.state.idle) {
            this.store.set({ idle: false });
        }

        if (! this.store.state.fullscreen) {
            return;
        }

        this.idleTimer = setTimeout(() => this.store.set({ idle: true }), Media.IDLE_MS);
    }

    tearDown(): Promise<void> {
        this.tearingDown = this.tearingDown
            .then(() => this.release())
            .catch((failure: unknown) => this.app.log('media.teardown.error', { message: Failure.message(failure) }));

        return this.tearingDown;
    }

    async release(): Promise<void> {
        this.sfu?.leaveRoom();
        this.sfu?.disconnect();
        await this.app.sharing.stop();
        clearInterval(this.peopleStatsTimer ?? undefined);
        this.peopleStatsTimer = null;

        const fullscreen = this.store.state.fullscreen;

        this.sfu = null;
        this.broadcast = null;
        this.nativeWatching.clear();
        await Tauri.invoke('stop_watch', { producerId: null }).catch((failure: unknown) => this.app.log('media.native.stop.error', { message: Failure.message(failure) }));

        for (const audio of [...this.remoteAudios.values(), ...this.micAudios.values()]) {
            (audio.srcObject as MediaStream | null)?.getTracks?.().forEach(track => track.stop());
            audio.remove();
        }

        for (const tile of this.store.state.tiles) {
            tile.stream?.getTracks?.().forEach(track => track.stop());
        }

        for (const timer of this.mediaStatsTimers.values()) {
            clearInterval(timer);
        }

        this.remoteAudios.clear();
        this.micAudios.clear();
        this.consumerSources.clear();
        this.hiddenPeers.clear();
        this.mediaStatsTimers.clear();
        this.videos.clear();
        this.deafened = false;
        this.stats.replace({});
        this.store.replace({ ...this.initialState(), image: this.store.state.image });

        if (fullscreen) {
            await Tauri.setFullscreen(false)?.catch?.((failure: unknown) => this.app.log('media.fullscreen.error', { message: Failure.message(failure) }));
        }
    }
}
