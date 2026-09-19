import { Store } from './Store.ts';

export type MicState = {
    level: number;
    speaking: boolean;
};

export class Mic {
    static readonly FLOOR_DB = -70;
    static readonly TAIL_MS = 350;
    static readonly TICK_MS = 50;
    static readonly FFT_SIZE = 1024;
    static readonly SILENT_MS = 12_000;
    static readonly FLOOR_LEVEL = 2;
    static readonly STALL_MS = 1500;

    readonly store = new Store<MicState>({ level: 0, speaking: false });
    threshold = 40;
    private context: AudioContext | null = null;
    private analyser: AnalyserNode | null = null;
    private source: MediaStreamAudioSourceNode | null = null;
    private probe: MediaStreamTrack | null = null;
    private samples: Float32Array<ArrayBuffer> | null = null;
    private timer: ReturnType<typeof setInterval> | null = null;
    private spokeAt = 0;
    private startedAt = 0;
    private external = false;
    private fedAt = 0;
    private warnedSilence = false;
    private onSpeaking: (speaking: boolean) => void = () => undefined;
    private onSilence: () => void = () => undefined;
    private onFailure: (stage: string, failure: unknown) => void = () => undefined;

    static levelOf(samples: Float32Array): number {
        let sum = 0;

        for (const sample of samples) {
            sum += sample * sample;
        }

        return Mic.levelOfPower(sum / samples.length);
    }

    static levelOfRms(rms: number): number {
        return Mic.levelOfPower(rms * rms);
    }

    static levelOfPower(power: number): number {
        const decibels = 10 * Math.log10(Math.max(power, 1e-12));

        return Math.round(Math.min(100, Math.max(0, (decibels - Mic.FLOOR_DB) * (100 / -Mic.FLOOR_DB))));
    }

    onError(handler: (stage: string, failure: unknown) => void): void {
        this.onFailure = handler;
    }

    watch(track: MediaStreamTrack, threshold: number, onSpeaking: (speaking: boolean) => void, onSilence: () => void = () => undefined): void {
        this.arm(threshold, onSpeaking, onSilence);
        this.context = new AudioContext();

        if (this.context.state === 'suspended') {
            this.context.resume().catch((failure: unknown) => this.onFailure('resume', failure));
        }

        this.analyser = this.context.createAnalyser();
        this.analyser.fftSize = Mic.FFT_SIZE;
        this.samples = new Float32Array(this.analyser.fftSize);
        this.probe = track.clone();
        this.probe.enabled = true;
        this.source = this.context.createMediaStreamSource(new MediaStream([this.probe]));
        this.source.connect(this.analyser);
        this.timer = setInterval(() => this.tick(), Mic.TICK_MS);
    }

    watchLevels(threshold: number, onSpeaking: (speaking: boolean) => void, onSilence: () => void = () => undefined): void {
        this.arm(threshold, onSpeaking, onSilence);
        this.external = true;
        this.timer = setInterval(() => this.checkStall(), Mic.STALL_MS / 3);
    }

    feed(rms: number): void {
        if (! this.external) {
            return;
        }

        const first = this.fedAt === 0;

        this.fedAt = Date.now();
        this.measure(Mic.levelOfRms(rms), first);
    }

    setThreshold(threshold: number): void {
        this.threshold = threshold;
    }

    stop(): void {
        if (this.timer) {
            clearInterval(this.timer);
            this.timer = null;
        }

        this.source?.disconnect();
        this.probe?.stop();
        this.probe = null;
        this.analyser?.disconnect();
        this.context?.close().catch((failure: unknown) => this.onFailure('close', failure));
        this.context = null;
        this.analyser = null;
        this.source = null;
        this.samples = null;
        this.external = false;
        this.fedAt = 0;
        this.store.set({ level: 0, speaking: false });
    }

    private arm(threshold: number, onSpeaking: (speaking: boolean) => void, onSilence: () => void): void {
        this.stop();
        this.threshold = threshold;
        this.onSpeaking = onSpeaking;
        this.onSilence = onSilence;
        this.startedAt = Date.now();
        this.spokeAt = 0;
        this.warnedSilence = false;
    }

    private checkStall(): void {
        if (this.fedAt === 0 || Date.now() - this.fedAt < Mic.STALL_MS) {
            return;
        }

        this.fedAt = 0;
        this.spokeAt = 0;
        this.store.set({ level: 0, speaking: false });
        this.onFailure('level', new Error('o nível do microfone parou de chegar: o microfone fica aberto'));
        this.onSpeaking(true);
    }

    private tick(): void {
        if (! this.analyser || ! this.samples) {
            return;
        }

        this.analyser.getFloatTimeDomainData(this.samples);
        this.measure(Mic.levelOf(this.samples));
    }

    private measure(level: number, first = false): void {
        const now = Date.now();

        if (level >= this.threshold) {
            this.spokeAt = now;
        }

        const speaking = this.spokeAt > 0 && now - this.spokeAt < Mic.TAIL_MS;

        if (first || speaking !== this.store.state.speaking) {
            this.onSpeaking(speaking);
        }

        if (! this.warnedSilence && this.spokeAt === 0 && level <= Mic.FLOOR_LEVEL && now - this.startedAt > Mic.SILENT_MS) {
            this.warnedSilence = true;
            this.onSilence();
        }

        this.store.set({ level, speaking });
    }
}
