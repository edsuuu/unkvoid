/**
 * Owns the microphone: capture, gating and the input meter.
 *
 * The gate is `track.enabled`, never `producer.pause()` or a new `getUserMedia`.
 * Opening and closing a producer on every syllable renegotiates the transport dozens
 * of times a minute, and re-acquiring the device blinks the operating system's
 * microphone indicator. A disabled track keeps the session alive and emits silence,
 * which Opus turns into almost no bytes.
 *
 * What is published is a clone of the captured track; the meter reads the original.
 * See `open()` — measuring the gated track is a deadlock, not a detail.
 */
export class MicrophoneGate {
    static SETTINGS_KEY = 'voice:mic';

    static DEFAULTS = {
        mode: 'voice',
        threshold: -50,
        pushKey: 'ControlLeft',
        noiseSuppression: true,
    };

    /** Floor of the meter in dBFS: below this everything is indistinguishable from silence. */
    static FLOOR_DB = -100;

    /**
     * A partir daqui a pessoa é considerada falando, para efeito da borda no avatar.
     *
     * É fixo e mais alto que o limiar do gate: o limiar de quem envia é escolha dela
     * ("o que sai da minha máquina"), e o indicador é sobre o que já chegou aqui.
     */
    static SPEAKING_DB = -45;

    /** How often the level is reported. Comfortably faster than a syllable. */
    static FRAME_MS = 50;

    /**
     * The meter runs on the audio thread, not on a timer. A window in the background has
     * `setTimeout` clamped to one second — measured here: 60 ms became 1000 ms, while
     * this worklet kept delivering every 11 ms in the same window. On a timer, minimising
     * the app would chop a second off the start of every sentence.
     */
    static METER = `
        class Meter extends AudioWorkletProcessor {
            constructor(options) {
                super();
                this.block = options.processorOptions.block;
                this.sum = 0;
                this.count = 0;
            }

            process(inputs) {
                const channel = inputs[0]?.[0];

                if (channel) {
                    for (let i = 0; i < channel.length; i++) {
                        this.sum += channel[i] * channel[i];
                    }
                }

                // No input still counts as time passing, otherwise a silent microphone
                // would never report and the gate would stay stuck open.
                this.count += channel?.length ?? 128;

                if (this.count >= this.block) {
                    this.port.postMessage(Math.sqrt(this.sum / this.count));
                    this.sum = 0;
                    this.count = 0;
                }

                return true;
            }
        }

        registerProcessor('unkvoid-meter', Meter);
    `;

    /**
     * The gate stays open this long after the voice drops. Without it every pause
     * between words cuts the transmission and the speech arrives chopped.
     */
    static RELEASE_MS = 300;

    constructor(onChange) {
        this.settings = MicrophoneGate.load();
        this.onChange = onChange ?? (() => {});
        this.stream = null;
        this.source = null;
        this.track = null;
        this.context = null;
        this.meter = null;
        this.openUntil = 0;
        this.pushing = false;
        this.muted = false;
        this.db = MicrophoneGate.FLOOR_DB;

        this.onKeyDown = event => this.pressed(event, true);
        this.onKeyUp = event => this.pressed(event, false);

        // Losing focus never delivers the keyup: without this, releasing the key outside
        // the window would leave the microphone open for good.
        this.onBlur = () => {
            this.pushing = false;
            this.apply();
        };
    }

    /**
     * Observa o nível de um stream sem gatear nada — para saber quem está falando.
     *
     * Reusa o mesmo medidor do microfone de propósito: é a mesma pergunta ("está saindo
     * som?") feita sobre o áudio de outra pessoa, e um segundo medidor com outro limiar
     * daria respostas diferentes para a mesma voz.
     *
     * Devolve a função que desliga tudo.
     */
    static async watch(stream, onSpeaking) {
        const context = new AudioContext();

        await context.resume().catch(() => {});

        const module = URL.createObjectURL(new Blob([MicrophoneGate.METER], { type: 'application/javascript' }));

        try {
            await context.audioWorklet.addModule(module);
        } finally {
            URL.revokeObjectURL(module);
        }

        const meter = new AudioWorkletNode(context, 'unkvoid-meter', {
            processorOptions: { block: Math.round(MicrophoneGate.FRAME_MS / 1000 * context.sampleRate) },
        });

        let speaking = false;
        let until = 0;

        meter.port.onmessage = event => {
            const db = event.data > 0
                ? Math.max(MicrophoneGate.FLOOR_DB, 20 * Math.log10(event.data))
                : MicrophoneGate.FLOOR_DB;

            if (db >= MicrophoneGate.SPEAKING_DB) {
                until = performance.now() + MicrophoneGate.RELEASE_MS;
            }

            const agora = performance.now() < until;

            // Só avisa na virada: pintar a borda 20 vezes por segundo é trabalho à toa.
            if (agora !== speaking) {
                speaking = agora;
                onSpeaking(agora);
            }
        };

        const silence = context.createGain();

        silence.gain.value = 0;
        context.createMediaStreamSource(stream).connect(meter).connect(silence).connect(context.destination);

        return () => {
            meter.port.close();
            context.close().catch(() => {});
        };
    }

    static load() {
        try {
            return { ...MicrophoneGate.DEFAULTS, ...JSON.parse(localStorage.getItem(MicrophoneGate.SETTINGS_KEY) ?? '{}') };
        } catch {
            return { ...MicrophoneGate.DEFAULTS };
        }
    }

    save(changes) {
        this.settings = { ...this.settings, ...changes };

        try {
            localStorage.setItem(MicrophoneGate.SETTINGS_KEY, JSON.stringify(this.settings));
        } catch {
            // Browser without storage: the settings just do not survive a reload.
        }

        if ('noiseSuppression' in changes && this.source) {
            // Applied to the live tracks, so switching does not drop the call — the same
            // trick the quality selector uses for the screen share. Both sides get it:
            // the clone is what is heard, the original is what the meter reads.
            const audio = this.constraints().audio;

            this.source.applyConstraints(audio).catch(() => {});
            this.track.applyConstraints(audio).catch(() => {});
        }

        this.apply();
    }

    constraints() {
        return {
            audio: {
                noiseSuppression: this.settings.noiseSuppression,
                echoCancellation: true,
                autoGainControl: true,
            },
        };
    }

    async open() {
        if (this.track) {
            return this.track;
        }

        this.stream = await navigator.mediaDevices.getUserMedia(this.constraints());
        this.source = this.stream.getAudioTracks()[0];

        // The published track is a CLONE, and the meter listens to the original. A clone
        // carries its own `enabled`, so closing the gate silences what goes out without
        // silencing what is measured. Gating the measured track deadlocks: it reads zero
        // the instant it closes, so the level never comes back above the threshold and
        // the microphone never opens again.
        this.track = this.source.clone();

        this.context = new AudioContext();

        // A context created outside a gesture starts suspended, and a suspended graph
        // never runs the meter — the level would sit at the floor and voice activity
        // would never open the gate. Joining is a click, so this resolves at once.
        await this.context.resume().catch(() => {});

        const module = URL.createObjectURL(new Blob([MicrophoneGate.METER], { type: 'application/javascript' }));

        try {
            await this.context.audioWorklet.addModule(module);
        } finally {
            URL.revokeObjectURL(module);
        }

        this.meter = new AudioWorkletNode(this.context, 'unkvoid-meter', {
            processorOptions: { block: Math.round(MicrophoneGate.FRAME_MS / 1000 * this.context.sampleRate) },
        });
        this.meter.port.onmessage = event => this.observe(event.data);

        const silence = this.context.createGain();
        silence.gain.value = 0;

        // Pulled all the way to the destination so the graph is never culled — and at
        // zero gain, so nobody hears their own microphone.
        this.context.createMediaStreamSource(this.stream).connect(this.meter).connect(silence).connect(this.context.destination);

        window.addEventListener('keydown', this.onKeyDown);
        window.addEventListener('keyup', this.onKeyUp);
        window.addEventListener('blur', this.onBlur);

        this.apply();

        return this.track;
    }

    close() {
        this.meter?.port.close();
        window.removeEventListener('keydown', this.onKeyDown);
        window.removeEventListener('keyup', this.onKeyUp);
        window.removeEventListener('blur', this.onBlur);

        this.track?.stop();
        this.source?.stop();
        this.context?.close().catch(() => {});

        this.meter = null;
        this.track = null;
        this.source = null;
        this.stream = null;
        this.context = null;
        this.db = MicrophoneGate.FLOOR_DB;
        this.pushing = false;
        this.openUntil = 0;
    }

    get active() {
        return Boolean(this.track);
    }

    setMuted(muted) {
        this.muted = muted;
        this.apply();
    }

    /**
     * One RMS reading from the audio thread. RMS and not peak: a single click should not
     * open the gate, and the eye reads an average far better than a spike.
     */
    observe(rms) {
        this.db = rms > 0
            ? Math.max(MicrophoneGate.FLOOR_DB, 20 * Math.log10(rms))
            : MicrophoneGate.FLOOR_DB;

        if (this.settings.mode === 'voice' && this.db >= this.settings.threshold) {
            this.openUntil = performance.now() + MicrophoneGate.RELEASE_MS;
        }

        this.apply();
    }

    /** True when the audio should actually leave this machine. */
    get transmitting() {
        if (this.muted) {
            return false;
        }

        return this.settings.mode === 'ptt'
            ? this.pushing
            : performance.now() < this.openUntil;
    }

    apply() {
        if (! this.track) {
            return;
        }

        const transmitting = this.transmitting;

        if (this.track.enabled !== transmitting) {
            this.track.enabled = transmitting;
        }

        this.onChange({ db: this.db, transmitting, muted: this.muted });
    }

    /**
     * Push to talk reads `event.code`, not `event.key`: the physical key has to work
     * with any keyboard layout, and holding a modifier changes what `key` reports.
     */
    pressed(event, down) {
        if (this.settings.mode !== 'ptt' || event.code !== this.settings.pushKey) {
            return;
        }

        if (down && MicrophoneGate.typing(event.target)) {
            return;
        }

        if (this.pushing === down) {
            return;
        }

        this.pushing = down;
        this.apply();
    }

    static typing(element) {
        return element instanceof HTMLElement
            && (element.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(element.tagName));
    }

    /** Fraction 0..1 of the meter, so the bar and the threshold share one scale. */
    static toFraction(db) {
        return Math.min(1, Math.max(0, (db - MicrophoneGate.FLOOR_DB) / -MicrophoneGate.FLOOR_DB));
    }
}
