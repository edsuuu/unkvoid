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

    readonly store = new Store<MicState>({ level: 0, speaking: false });
    threshold = 40;
    private context: AudioContext | null = null;
    private analyser: AnalyserNode | null = null;
    private source: MediaStreamAudioSourceNode | null = null;
    private samples: Float32Array<ArrayBuffer> | null = null;
    private timer: ReturnType<typeof setInterval> | null = null;
    private spokeAt = 0;
    private onSpeaking: (speaking: boolean) => void = () => undefined;

    static levelOf(samples: Float32Array): number {
        let sum = 0;

        for (const sample of samples) {
            sum += sample * sample;
        }

        const decibels = 10 * Math.log10(Math.max(sum / samples.length, 1e-12));

        return Math.round(Math.min(100, Math.max(0, (decibels - Mic.FLOOR_DB) * (100 / -Mic.FLOOR_DB))));
    }

    watch(track: MediaStreamTrack, threshold: number, onSpeaking: (speaking: boolean) => void): void {
        this.stop();
        this.threshold = threshold;
        this.onSpeaking = onSpeaking;
        this.context = new AudioContext();

        if (this.context.state === 'suspended') {
            void this.context.resume();
        }

        this.analyser = this.context.createAnalyser();
        this.analyser.fftSize = Mic.FFT_SIZE;
        this.samples = new Float32Array(this.analyser.fftSize);
        this.source = this.context.createMediaStreamSource(new MediaStream([track]));
        this.source.connect(this.analyser);
        this.timer = setInterval(() => this.tick(), Mic.TICK_MS);
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
        this.analyser?.disconnect();
        void this.context?.close();
        this.context = null;
        this.analyser = null;
        this.source = null;
        this.samples = null;
        this.store.set({ level: 0, speaking: false });
    }

    private tick(): void {
        if (! this.analyser || ! this.samples) {
            return;
        }

        this.analyser.getFloatTimeDomainData(this.samples);

        const level = Mic.levelOf(this.samples);
        const now = Date.now();

        if (level >= this.threshold) {
            this.spokeAt = now;
        }

        const speaking = this.spokeAt > 0 && now - this.spokeAt < Mic.TAIL_MS;

        if (speaking !== this.store.state.speaking) {
            this.onSpeaking(speaking);
        }

        this.store.set({ level, speaking });
    }
}
