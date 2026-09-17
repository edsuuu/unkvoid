type Step = { hertz: number; startsAt: number; seconds?: number };

export class Sounds {
    static readonly VOLUME = 0.07;
    static readonly STEP_SECONDS = 0.09;

    private context: AudioContext | null = null;

    joined(): void {
        this.play([{ hertz: 523, startsAt: 0 }, { hertz: 784, startsAt: 0.08 }]);
    }

    left(): void {
        this.play([{ hertz: 659, startsAt: 0 }, { hertz: 440, startsAt: 0.08 }]);
    }

    streamStarted(): void {
        this.play([{ hertz: 587, startsAt: 0 }, { hertz: 740, startsAt: 0.08 }, { hertz: 880, startsAt: 0.16 }]);
    }

    streamStopped(): void {
        this.play([{ hertz: 880, startsAt: 0 }, { hertz: 740, startsAt: 0.08 }, { hertz: 587, startsAt: 0.16 }]);
    }

    message(): void {
        this.play([{ hertz: 988, startsAt: 0, seconds: 0.06 }, { hertz: 1319, startsAt: 0.05, seconds: 0.1 }]);
    }

    muted(): void {
        this.play([{ hertz: 494, startsAt: 0, seconds: 0.07 }, { hertz: 370, startsAt: 0.06, seconds: 0.07 }]);
    }

    unmuted(): void {
        this.play([{ hertz: 370, startsAt: 0, seconds: 0.07 }, { hertz: 587, startsAt: 0.06, seconds: 0.07 }]);
    }

    deafened(): void {
        this.play([{ hertz: 440, startsAt: 0, seconds: 0.07 }, { hertz: 330, startsAt: 0.06, seconds: 0.07 }, { hertz: 247, startsAt: 0.12, seconds: 0.09 }]);
    }

    undeafened(): void {
        this.play([{ hertz: 247, startsAt: 0, seconds: 0.07 }, { hertz: 330, startsAt: 0.06, seconds: 0.07 }, { hertz: 440, startsAt: 0.12, seconds: 0.09 }]);
    }

    private play(steps: Step[]): void {
        const context = this.open();

        if (! context) {
            return;
        }

        for (const step of steps) {
            const startsAt = context.currentTime + step.startsAt;
            const seconds = step.seconds ?? Sounds.STEP_SECONDS;
            const oscillator = context.createOscillator();
            const gain = context.createGain();

            oscillator.type = 'sine';
            oscillator.frequency.setValueAtTime(step.hertz, startsAt);

            gain.gain.setValueAtTime(0, startsAt);
            gain.gain.linearRampToValueAtTime(Sounds.VOLUME, startsAt + 0.012);
            gain.gain.exponentialRampToValueAtTime(0.0001, startsAt + seconds);

            oscillator.connect(gain).connect(context.destination);
            oscillator.start(startsAt);
            oscillator.stop(startsAt + seconds + 0.02);
        }
    }

    private open(): AudioContext | null {
        if (typeof AudioContext === 'undefined') {
            return null;
        }

        this.context ??= new AudioContext();

        if (this.context.state === 'suspended') {
            void this.context.resume();
        }

        return this.context;
    }
}
