import type { App } from './App.ts';
import { Failure } from './Failure.ts';
import type { Hub } from './Hub.ts';
import { Media } from './Media.ts';
import { Mic } from './Mic.ts';
import type { Channel } from './Models.ts';
import { Platform } from './Platform.ts';
import { SfuClient, type JoinResponse, type PlainProducerResponse, type RoomIdentity, type SourceName } from './SfuClient.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';

export type InputMode = 'voice' | 'ptt' | 'open';

export type Keybinds = {
    mute: string;
    deafen: string;
    talk: string;
};

export type VoicePreferences = {
    microphone: string;
    noiseSuppression: boolean;
    muteOnJoin: boolean;
    inputMode: InputMode;
    sensitivity: number;
    keybinds: Keybinds;
};

export type VoiceState = {
    channel: Channel | null;
    joining: boolean;
    muted: boolean;
    deafened: boolean;
    serverMuted: boolean;
    can: string[];
    cameraOn: boolean;
    speaking: boolean;
    talkKeyRefused: boolean;
    micProblem: string;
    preferences: VoicePreferences;
};

type Camera = { id: string };

export class Voice {
    static readonly MIC_OPTIONS = { codecOptions: { opusDtx: true, opusFec: true } };
    static readonly CAMERA_OPTIONS = { encodings: [{ maxBitrate: 1_200_000 }], codecOptions: { videoGoogleStartBitrate: 800 } };
    static readonly PREFERENCES_KEY = 'unkvoid:voice';
    static readonly DEFAULT_KEYBINDS: Keybinds = { mute: 'CmdOrCtrl+Shift+KeyM', deafen: 'CmdOrCtrl+Shift+KeyD', talk: '' };
    static readonly DEFAULT_PREFERENCES: VoicePreferences = {
        microphone: '',
        noiseSuppression: true,
        muteOnJoin: true,
        inputMode: 'voice',
        sensitivity: 35,
        keybinds: Voice.DEFAULT_KEYBINDS,
    };

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
    leaving: Promise<void> = Promise.resolve();
    gateOpen = true;
    listening = false;
    registeredShortcuts = new Set<string>();
    readonly mic = new Mic();
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
            speaking: false,
            talkKeyRefused: false,
            micProblem: '',
            preferences: { ...preferences, keybinds: { ...Voice.DEFAULT_KEYBINDS, ...preferences.keybinds } },
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

    watchMicErrors(): void {
        this.mic.onError((stage, failure) => this.app.log('voice.detection.error', { stage, message: Failure.message(failure) }));
    }

    listenShortcuts(): void {
        if (this.listening || ! Tauri.available()) {
            return;
        }

        this.listening = true;

        void Tauri.listen<{ action: string; pressed: boolean }>('shortcut', ({ payload }) => this.onShortcut(payload.action, payload.pressed));
    }

    async applyShortcuts(): Promise<void> {
        if (! Tauri.available()) {
            return;
        }

        const { keybinds, inputMode } = this.store.state.preferences;
        const bindings = [
            { action: 'mute', accelerator: keybinds.mute },
            { action: 'deafen', accelerator: keybinds.deafen },
            { action: 'talk', accelerator: inputMode === 'ptt' ? keybinds.talk : '' },
        ].filter(binding => binding.accelerator.trim() !== '');

        try {
            const result = await Tauri.invoke<{ registered: string[]; failed: string[] }>('set_shortcuts', { bindings });

            this.registeredShortcuts = new Set(result.registered);
            this.store.set({ talkKeyRefused: result.failed.includes('talk') });

            if (result.failed.length > 0) {
                this.app.log('voice.shortcuts.refused', { failed: result.failed });
                this.app.toast(
                    result.failed.includes('talk')
                        ? 'outro programa já usa a tecla de falar: o seu microfone fica aberto até você escolher outra'
                        : `o sistema recusou ${result.failed.length === 1 ? 'uma tecla' : 'algumas teclas'}: outro programa já a usa. Escolha outra.`,
                    true,
                );
            }
        } catch (failure) {
            this.registeredShortcuts = new Set();
            this.store.set({ talkKeyRefused: this.store.state.preferences.inputMode === 'ptt' });
            this.app.log('voice.shortcuts.error', { message: Failure.message(failure) });
            this.app.toast(`não deu para registrar os atalhos: ${Failure.message(failure)}`, true);
        }

        if (this.pushToTalk()) {
            this.setGate(false);
        }
    }

    onShortcut(action: string, pressed: boolean): void {
        if (action === 'talk') {
            if (this.pushToTalk()) {
                this.setGate(pressed);
            }

            return;
        }

        if (! pressed) {
            return;
        }

        if (action === 'mute') {
            void this.hub.attempt(() => this.toggleMute());
        }

        if (action === 'deafen') {
            void this.hub.attempt(() => this.toggleDeafen());
        }
    }

    allowed(grant: string): boolean {
        return this.can.includes(grant);
    }

    async join(channel: Channel): Promise<void> {
        await this.leaving;

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
        this.publish({ joining: true });

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
            sfu.on('kicked', () => this.leaveIfCurrent(sfu));
            sfu.on('replaced', () => this.leaveIfCurrent(sfu));
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
                this.app.sounds.joined();
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

    leaveIfCurrent(sfu: SfuClient): void {
        if (this.app.media.sfu === sfu) {
            void this.leave();
        }
    }

    leave(): Promise<void> {
        const channel = this.channel;

        if (! channel) {
            return this.leaving;
        }

        this.channel = null;
        this.joinTicket += 1;
        this.leaving = this.release(channel).catch((failure: unknown) => this.app.log('voice.leave.error', { channel: channel.id, message: Failure.message(failure) }));

        return this.leaving;
    }

    async release(channel: Channel): Promise<void> {
        this.app.sounds.left();
        this.app.log('voice.leave', { channel: channel.id });
        await this.stopCamera().catch((failure: unknown) => this.app.log('voice.camera.stop.error', { message: Failure.message(failure) }));
        await this.stopMic().catch((failure: unknown) => this.app.log('voice.mic.stop.error', { message: Failure.message(failure) }));
        await this.app.media.tearDown();
        this.can = [];
        this.deafened = false;
        this.serverMuted = false;
        this.publish({ joining: false });
        this.hub.publish({ stageOpen: false, focusedRoom: false });
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

            const track = this.micTrack ?? await this.openMicrophone(audio);

            if (! track) {
                return;
            }

            if (ticket !== this.joinTicket) {
                track.stop();

                return;
            }

            this.micTrack = track;
            this.micTrack.enabled = ! (this.muted || this.serverMuted);
            this.micProducerId = (await this.app.media.sfu!.produce(this.micTrack, 'mic', Voice.MIC_OPTIONS)).id;
        }

        this.startGate();

        if (this.muted || this.serverMuted) {
            await this.applyMute();
        }

        this.app.log('voice.mic.started', { producerId: this.micProducerId, native: this.native() });
    }

    async openMicrophone(audio: MediaTrackConstraints): Promise<MediaStreamTrack | null> {
        try {
            const track = (await navigator.mediaDevices.getUserMedia({ audio })).getAudioTracks()[0];

            this.store.set({ micProblem: '' });

            return track ?? null;
        } catch (failure) {
            const problem = Voice.micProblem(failure);

            this.app.log('voice.mic.unavailable', { name: (failure as DOMException | null)?.name ?? null, message: Failure.message(failure) });
            this.store.set({ micProblem: problem });
            this.app.toast(problem, true);

            return null;
        }
    }

    static micProblem(failure: unknown): string {
        switch ((failure as DOMException | null)?.name) {
            case 'NotAllowedError':
            case 'SecurityError':
                return 'o sistema não liberou o microfone: autorize nas configurações de privacidade e entre de novo';
            case 'NotReadableError':
            case 'AbortError':
                return 'outro programa está segurando o microfone: feche ele e entre de novo';
            case 'OverconstrainedError':
                return 'o microfone escolhido não existe mais: escolha outro nas configurações';
            default:
                return 'nenhum microfone encontrado: você entrou só para ouvir';
        }
    }

    async stopMic(): Promise<void> {
        if (this.micProducerId) {
            await this.app.media.sfu?.closeProducer(this.micProducerId);
            this.micProducerId = null;
        }

        this.mic.stop();
        this.micTrack?.stop();
        this.micTrack = null;
        this.gateOpen = true;
        this.store.set({ speaking: false, micProblem: '' });

        if (this.native()) {
            await Tauri.invoke('stop_voice').catch((failure: unknown) => this.app.log('voice.stop.error', { message: Failure.message(failure) }));
        }
    }

    pushToTalk(): boolean {
        return this.store.state.preferences.inputMode === 'ptt' && this.registeredShortcuts.has('talk');
    }

    startGate(): void {
        const { inputMode, sensitivity } = this.store.state.preferences;

        this.mic.stop();
        this.gateOpen = ! this.pushToTalk() && (inputMode !== 'voice' || ! this.micTrack);

        if (this.micTrack) {
            try {
                this.mic.watch(
                    this.micTrack,
                    sensitivity,
                    speaking => {
                        if (this.store.state.preferences.inputMode === 'voice') {
                            this.setGate(speaking);
                        }
                    },
                    () => {
                        if (this.store.state.preferences.inputMode === 'voice' && ! this.muted && ! this.serverMuted) {
                            this.app.log('voice.detection.silent', { microphone: this.store.state.preferences.microphone });
                            this.app.toast('seu microfone não captou nada até agora: confira o botão do fone e a entrada nas configurações', true);
                        }
                    },
                );
            } catch (failure) {
                this.app.log('voice.detection.error', { message: Failure.message(failure) });
                this.app.toast('a detecção de voz não abriu neste sistema: o microfone fica sempre aberto', true);
                this.gateOpen = true;
            }
        }

        this.applyGate();
    }

    setGate(open: boolean): void {
        if (this.gateOpen === open) {
            return;
        }

        this.gateOpen = open;
        this.applyGate();
    }

    applyGate(): void {
        const live = ! this.muted && ! this.serverMuted && this.gateOpen;

        if (this.micTrack) {
            this.micTrack.enabled = live;
        }

        if (this.native() && this.micProducerId) {
            void Tauri.invoke('set_voice_muted', { muted: ! live }).catch((failure: unknown) => {
                this.app.log('voice.gate.error', { message: Failure.message(failure), live });
                this.app.toast(
                    live
                        ? 'o microfone não abriu: ninguém está te ouvindo. Saia e entre na voz de novo.'
                        : 'o microfone não fechou: o áudio pode estar saindo mesmo com o botão mutado',
                    true,
                );
            });
        }

        this.store.set({ speaking: live });
    }

    async toggleMute(): Promise<void> {
        if (this.serverMuted || ! this.allowed('speak')) {
            return;
        }

        this.muted = ! this.muted;
        this.app.sounds[this.muted ? 'muted' : 'unmuted']();
        this.publish();

        await this.hub.attempt(() => (this.micProducerId ? this.applyMute() : this.startMic()));
    }

    async applyMute(): Promise<void> {
        const muted = this.muted || this.serverMuted;

        this.applyGate();

        if (this.micProducerId) {
            if (this.serverMuted) {
                const producer = this.app.media.sfu!.producers.get(this.micProducerId);

                if (producer) {
                    producer.pause();
                } else {
                    this.app.log('voice.servermute.missing', { producerId: this.micProducerId });
                }
            } else {
                await this.app.media.sfu![muted ? 'pauseProducer' : 'resumeProducer'](this.micProducerId);
            }
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
        this.app.sounds[this.deafened ? 'deafened' : 'undeafened']();
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

        if (key === 'keybinds') {
            await this.applyShortcuts();
            this.startGate();

            return;
        }

        if (key === 'inputMode') {
            await this.applyShortcuts();
        }

        if (key === 'sensitivity') {
            this.mic.setThreshold(preferences.sensitivity);

            return;
        }

        if (key === 'inputMode') {
            this.startGate();

            return;
        }

        if (key === 'muteOnJoin' || ! this.micTrack || ! this.channel || this.native()) {
            return;
        }

        await this.hub.attempt(async () => {
            await this.stopMic();
            await this.startMic();
        });
    }
}
