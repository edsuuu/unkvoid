import type { App } from './App.ts';
import { Failure } from './Failure.ts';
import type { Hub } from './Hub.ts';
import { Media } from './Media.ts';
import type { Channel, Clip } from './Models.ts';
import { Platform } from './Platform.ts';
import { SfuClient, type JoinResponse, type PlainProducerResponse, type RoomIdentity, type SourceName } from './SfuClient.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';

export type VoicePreferences = {
    microphone: string;
    noiseSuppression: boolean;
    muteOnJoin: boolean;
};

export type Streamer = { userId: number; name: string };

export type VoiceState = {
    channel: Channel | null;
    joining: boolean;
    muted: boolean;
    deafened: boolean;
    serverMuted: boolean;
    can: string[];
    cameraOn: boolean;
    clipOpen: boolean;
    preferences: VoicePreferences;
};

type Camera = { id: string };

export class Voice {
    static readonly MIC_OPTIONS = { codecOptions: { opusDtx: true, opusFec: true } };
    static readonly CAMERA_OPTIONS = { encodings: [{ maxBitrate: 1_200_000 }], codecOptions: { videoGoogleStartBitrate: 800 } };
    static readonly PREFERENCES_KEY = 'unkvoid:voice';
    static readonly DEFAULT_PREFERENCES: VoicePreferences = { microphone: '', noiseSuppression: true, muteOnJoin: true };

    readonly app: App;
    readonly hub: Hub;
    channel: Channel | null = null;
    muted = false;
    deafened = false;
    serverMuted = false;
    micProducerId: string | null = null;
    micTrack: MediaStreamTrack | null = null;
    cameraProducerId: string | null = null;
    cameraTrack: MediaStreamTrack | null = null;
    can: string[] = [];
    joinTicket = 0;
    readonly store: Store<VoiceState>;

    constructor(app: App, hub: Hub) {
        this.app = app;
        this.hub = hub;

        let preferences: VoicePreferences = { ...Voice.DEFAULT_PREFERENCES };

        try {
            preferences = { ...preferences, ...(JSON.parse(localStorage.getItem(Voice.PREFERENCES_KEY) ?? '{}') as Partial<VoicePreferences>) };
        } catch (failure) {
            app.log('voice.preferences.error', { message: Failure.message(failure) });
        }

        this.store = new Store<VoiceState>({
            channel: null,
            joining: false,
            muted: false,
            deafened: false,
            serverMuted: false,
            can: [],
            cameraOn: false,
            clipOpen: false,
            preferences,
        });
    }

    publish(extra: Partial<VoiceState> = {}): void {
        this.store.set({
            channel: this.channel,
            muted: this.muted,
            deafened: this.deafened,
            serverMuted: this.serverMuted,
            can: this.can,
            cameraOn: Boolean(this.cameraProducerId),
            ...extra,
        });
    }

    native(): boolean {
        return Platform.isLinux();
    }

    allowed(grant: string): boolean {
        return this.can.includes(grant);
    }

    async join(channel: Channel): Promise<void> {
        if (this.channel?.id === channel.id) {
            return;
        }

        if (this.channel) {
            await this.leave();
        }

        const ticket = ++this.joinTicket;

        this.channel = channel;
        this.muted = this.store.state.preferences.muteOnJoin;
        this.can = [];
        this.serverMuted = Boolean(this.hub.me()?.server_mute);
        this.publish({ joining: true, clipOpen: false });

        try {
            const sfu = new SfuClient();

            sfu.on('reconnected', detail => {
                this.can = detail.can ?? this.can;
                this.publish();

                if (! detail.resumed) {
                    void this.republish();
                }
            });
            sfu.on('serverMuted', detail => void this.applyServerMute(detail.muted));
            sfu.on('kicked', () => void this.leave());
            sfu.on('replaced', () => void this.leave());
            sfu.on('peersChanged', () => this.hub.syncVoiceSources());

            await this.app.media.enterRoom(sfu, () => this.identity(channel), async (joined: JoinResponse) => {
                if (ticket !== this.joinTicket) {
                    return;
                }

                this.can = joined.can ?? [];
                this.publish();
                await this.startMic();
            });

            if (ticket === this.joinTicket) {
                this.publish({ joining: false });
                this.hub.syncVoiceSources();
            }
        } catch (failure) {
            this.app.log('voice.join.error', { channel: channel.id, message: Failure.message(failure) });

            if (ticket === this.joinTicket) {
                this.app.toast(`não deu para entrar na voz: ${Failure.message(failure)}`, true);
                await this.leave();
            }
        }
    }

    async identity(channel: Channel): Promise<RoomIdentity> {
        try {
            return { token: (await this.hub.api.post<{ token: string }>(`/api/channels/${channel.id}/voice/token`)).token };
        } catch (failure) {
            const status = Failure.status(failure);

            if (status === 401 || status === 403) {
                this.app.toast(status === 401 ? 'sua sessão expirou, entre de novo' : `não deu para entrar na voz: ${Failure.message(failure)}`, true);
                this.app.media.sfu?.disconnect();
                await this.leave();
            }

            if (status === 401) {
                await this.hub.logout();
            }

            throw failure;
        }
    }

    async leave(): Promise<void> {
        const channel = this.channel;

        if (! channel) {
            return;
        }

        this.channel = null;
        this.joinTicket += 1;
        this.app.log('voice.leave', { channel: channel.id });
        await this.stopCamera().catch((failure: unknown) => this.app.log('voice.camera.stop.error', { message: Failure.message(failure) }));
        await this.stopMic().catch((failure: unknown) => this.app.log('voice.mic.stop.error', { message: Failure.message(failure) }));
        await this.app.media.tearDown();
        this.can = [];
        this.deafened = false;
        this.serverMuted = false;
        this.publish({ joining: false, clipOpen: false });
        this.hub.publish({ stageOpen: false, focusedRoom: false });
    }

    streamers(): Streamer[] {
        const streamers: Streamer[] = [];

        if (! this.channel) {
            return streamers;
        }

        for (const peer of this.app.media.sfu?.peers?.values() ?? []) {
            if (peer.self && this.app.sharing.store.state.active) {
                streamers.push({ userId: this.hub.user!.id, name: `${this.hub.user!.name} (você)` });
            } else if (! peer.self && peer.sharing && peer.userId?.startsWith('user:')) {
                streamers.push({ userId: Number(peer.userId.slice('user:'.length)), name: peer.name });
            }
        }

        return streamers;
    }

    toggleClipList(): void {
        this.store.set(state => ({ clipOpen: ! state.clipOpen }));
    }

    closeEmptyClipList(): void {
        if (this.store.state.clipOpen && this.streamers().length === 0) {
            this.store.set({ clipOpen: false });
        }
    }

    async clip(streamer: Streamer): Promise<void> {
        const channel = this.channel;

        this.store.set({ clipOpen: false });

        const created = await this.hub.attempt(() => this.hub.api.post<Clip>(`/api/channels/${channel!.id}/clips`, { user_id: streamer.userId }));

        if (! created) {
            return;
        }

        this.app.toast('Clipando os últimos 5 min — vai aparecer na aba Clipes');
        this.hub.clips.update(created);
    }

    async publishNative(source: SourceName): Promise<string> {
        const offer = await Tauri.invoke<Record<string, unknown>>('sfu_offer', { source });
        const producer = await this.app.media.sfu!.request<PlainProducerResponse>('producePlain', {
            kind: source === 'mic' ? 'audio' : 'video',
            source,
            ...offer,
        });

        await Tauri.invoke('use_sfu', {
            address: `${producer.ip}:${producer.port}`,
            serverKey: producer.srtpParameters?.keyBase64 ?? null,
        });

        return producer.producerId;
    }

    async startMic(): Promise<void> {
        if (! this.allowed('speak') || this.micProducerId) {
            return;
        }

        const ticket = this.joinTicket;

        if (this.native()) {
            await Tauri.invoke('start_voice');

            if (ticket !== this.joinTicket) {
                await Tauri.invoke('stop_voice');

                return;
            }

            await Tauri.invoke('set_voice_muted', { muted: this.muted || this.serverMuted });
            this.micProducerId = await this.publishNative('mic');
        } else {
            const { microphone, noiseSuppression } = this.store.state.preferences;
            const audio: MediaTrackConstraints & { latency: number } = {
                deviceId: microphone ? { ideal: microphone } : undefined,
                echoCancellation: true,
                noiseSuppression,
                autoGainControl: true,
                latency: 0.01,
                channelCount: 1,
            };

            const track = this.micTrack ?? (await navigator.mediaDevices.getUserMedia({ audio })).getAudioTracks()[0];

            if (ticket !== this.joinTicket) {
                track.stop();

                return;
            }

            this.micTrack = track;
            this.micTrack.enabled = ! (this.muted || this.serverMuted);
            this.micProducerId = (await this.app.media.sfu!.produce(this.micTrack, 'mic', Voice.MIC_OPTIONS)).id;
        }

        if (this.muted || this.serverMuted) {
            await this.applyMute();
        }

        this.app.log('voice.mic.started', { producerId: this.micProducerId, native: this.native() });
    }

    async stopMic(): Promise<void> {
        if (this.micProducerId) {
            await this.app.media.sfu?.closeProducer(this.micProducerId);
            this.micProducerId = null;
        }

        this.micTrack?.stop();
        this.micTrack = null;

        if (this.native()) {
            await Tauri.invoke('stop_voice').catch((failure: unknown) => this.app.log('voice.stop.error', { message: Failure.message(failure) }));
        }
    }

    async toggleMute(): Promise<void> {
        if (this.serverMuted || ! this.allowed('speak')) {
            return;
        }

        this.muted = ! this.muted;
        this.publish();

        await this.hub.attempt(() => (this.micProducerId ? this.applyMute() : this.startMic()));
    }

    async applyMute(): Promise<void> {
        const muted = this.muted || this.serverMuted;

        if (this.micTrack) {
            this.micTrack.enabled = ! muted;
        }

        if (this.micProducerId) {
            if (this.serverMuted) {
                this.app.media.sfu!.producers.get(this.micProducerId)?.pause();
            } else {
                await this.app.media.sfu![muted ? 'pauseProducer' : 'resumeProducer'](this.micProducerId);
            }
        }

        if (this.native()) {
            await Tauri.invoke('set_voice_muted', { muted });
        }
    }

    async applyServerMute(muted: boolean): Promise<void> {
        this.serverMuted = muted;
        this.publish();

        if (! muted && ! this.micProducerId && ! this.allowed('speak') && this.channel) {
            const channel = this.channel;

            await this.leave();
            await this.join(channel);

            return;
        }

        await this.hub.attempt(() => this.applyMute());
    }

    async toggleDeafen(): Promise<void> {
        this.deafened = ! this.deafened;
        this.publish();
        await this.app.media.setDeafened(this.deafened);
    }

    async toggleCamera(): Promise<void> {
        await this.hub.attempt(() => this.cameraProducerId ? this.stopCamera() : this.startCamera());
        this.publish();
    }

    async startCamera(): Promise<void> {
        if (! this.allowed('video')) {
            return;
        }

        if (this.native()) {
            const cameras = await Tauri.invoke<Camera[]>('list_cameras');

            if (! cameras.length) {
                throw new Error('nenhuma câmera encontrada');
            }

            const [firstCamera] = cameras;

            await Tauri.invoke('start_camera', { device: firstCamera.id });

            try {
                this.cameraProducerId = await this.publishNative('camera');
            } catch (failure) {
                await this.stopCamera();

                throw failure;
            }

            return;
        }

        this.cameraTrack?.stop();

        const cameraTrack = (await navigator.mediaDevices.getUserMedia({
            video: { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 } },
        })).getVideoTracks()[0];

        this.cameraTrack = cameraTrack;

        try {
            this.cameraProducerId = (await this.app.media.sfu!.produce(cameraTrack, 'camera', Voice.CAMERA_OPTIONS)).id;
        } catch (failure) {
            await this.stopCamera();

            throw failure;
        }

        this.app.media.showScreen(Media.cameraKey(this.app.media.sfu!.peerId!), new MediaStream([cameraTrack]), 'camera');
    }

    async stopCamera(): Promise<void> {
        if (this.cameraProducerId) {
            await this.app.media.sfu?.closeProducer(this.cameraProducerId);
            this.cameraProducerId = null;
        }

        this.cameraTrack?.stop();
        this.cameraTrack = null;

        if (this.app.media.sfu?.peerId) {
            this.app.media.showScreen(Media.cameraKey(this.app.media.sfu.peerId), null);
        }

        if (this.native()) {
            await Tauri.invoke('stop_camera').catch((failure: unknown) => this.app.log('voice.camera.stop.error', { message: Failure.message(failure) }));
        }
    }

    async republish(): Promise<void> {
        const hadMic = Boolean(this.micProducerId) && this.allowed('speak');
        const hadCamera = Boolean(this.cameraProducerId) && this.allowed('video');

        this.micProducerId = null;
        this.cameraProducerId = null;

        try {
            if (hadMic && this.native()) {
                this.micProducerId = await this.publishNative('mic');
            } else if (hadMic && this.micTrack) {
                this.micProducerId = (await this.app.media.sfu!.produce(this.micTrack, 'mic', Voice.MIC_OPTIONS)).id;
            }

            if (this.muted || this.serverMuted) {
                await this.applyMute();
            }
        } catch (failure) {
            this.app.log('voice.republish.mic.error', { message: Failure.message(failure) });
            this.app.toast(`o microfone não voltou depois da reconexão: ${Failure.message(failure)}. Clique no microfone para tentar de novo.`, true);
        }

        try {
            if (hadCamera && this.native()) {
                this.cameraProducerId = await this.publishNative('camera');
            } else if (hadCamera && this.cameraTrack) {
                this.cameraProducerId = (await this.app.media.sfu!.produce(this.cameraTrack, 'camera')).id;
            }
        } catch (failure) {
            this.app.log('voice.republish.camera.error', { message: Failure.message(failure) });
            this.app.toast(`a câmera não voltou depois da reconexão: ${Failure.message(failure)}`, true);
            await this.stopCamera().catch((stopFailure: unknown) => this.app.log('voice.camera.stop.error', { message: Failure.message(stopFailure) }));
        }

        this.publish();
    }

    async setPreference<Key extends keyof VoicePreferences>(key: Key, value: VoicePreferences[Key]): Promise<void> {
        const preferences = { ...this.store.state.preferences, [key]: value };

        localStorage.setItem(Voice.PREFERENCES_KEY, JSON.stringify(preferences));
        this.store.set({ preferences });

        if (key === 'muteOnJoin' || ! this.micTrack || ! this.channel || this.native()) {
            return;
        }

        await this.hub.attempt(async () => {
            await this.stopMic();
            await this.startMic();
        });
    }
}
