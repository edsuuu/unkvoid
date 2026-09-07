/**
 * Runnable check for the gate: node resources/js/voice/MicrophoneGate.check.mjs
 *
 * It covers the decision, not the capture — what the gate does with a level, a
 * threshold, a held key and a mute. The browser part (getUserMedia, AudioContext)
 * is stubbed, because a wrong decision here is what makes someone inaudible.
 */
import assert from 'node:assert/strict';

let now = 0;

globalThis.localStorage = {
    store: {},
    getItem(key) { return this.store[key] ?? null; },
    setItem(key, value) { this.store[key] = value; },
};
globalThis.window = { addEventListener() {}, removeEventListener() {} };
globalThis.performance = { now: () => now };
globalThis.HTMLElement = class {};

const { MicrophoneGate } = await import('./MicrophoneGate.js');

/** A gate wired to a fake microphone, so `measure()` can be driven by hand. */
function gate(settings = {}) {
    const subject = new MicrophoneGate();

    subject.settings = { ...MicrophoneGate.DEFAULTS, ...settings };
    subject.track = { enabled: false };

    // The worklet reports RMS; dB is what the threshold speaks, so convert here.
    subject.speak = db => subject.observe(10 ** (db / 20));

    return subject;
}

// The meter and the threshold share one scale, otherwise the marker would lie.
assert.equal(MicrophoneGate.toFraction(0), 1);
assert.equal(MicrophoneGate.toFraction(-100), 0);
assert.equal(MicrophoneGate.toFraction(-50), 0.5);
assert.equal(MicrophoneGate.toFraction(-200), 0, 'below the floor it stays at zero, never negative');

// Voice activity: above the threshold transmits, below it does not.
const voice = gate({ mode: 'voice', threshold: -50 });

voice.speak(-60);
assert.equal(voice.track.enabled, false, 'quiet room does not open the gate');

voice.speak(-40);
assert.equal(voice.track.enabled, true, 'voice above the threshold opens it');

// The release window is what keeps words from being clipped between syllables.
now += MicrophoneGate.RELEASE_MS - 50;
voice.speak(-90);
assert.equal(voice.track.enabled, true, 'a pause inside the release window keeps transmitting');

now += 100;
voice.speak(-90);
assert.equal(voice.track.enabled, false, 'after the release window it closes');

// Mute beats everything, including a voice well above the threshold.
voice.speak(-10);
assert.equal(voice.track.enabled, true);
voice.setMuted(true);
assert.equal(voice.track.enabled, false, 'mute wins over voice activity');
voice.setMuted(false);
assert.equal(voice.track.enabled, true, 'unmuting inside the window transmits again');

// Push to talk ignores the level entirely.
const push = gate({ mode: 'ptt', pushKey: 'ControlLeft' });

push.speak(-5);
assert.equal(push.track.enabled, false, 'shouting does not transmit in push to talk');

push.pressed({ code: 'ControlLeft', target: null }, true);
assert.equal(push.track.enabled, true, 'holding the key transmits');

push.pressed({ code: 'ControlLeft', target: null }, false);
assert.equal(push.track.enabled, false, 'releasing the key stops it');

push.pressed({ code: 'KeyA', target: null }, true);
assert.equal(push.track.enabled, false, 'another key does nothing');

// Typing the bound key in a field must not open the microphone.
class FakeInput { constructor() { this.tagName = 'INPUT'; } }
Object.setPrototypeOf(FakeInput.prototype, globalThis.HTMLElement.prototype);

push.pressed({ code: 'ControlLeft', target: new FakeInput() }, true);
assert.equal(push.track.enabled, false, 'the key inside an input is typing, not talking');

// Losing focus never delivers the keyup, so blur has to release by itself.
push.pressed({ code: 'ControlLeft', target: null }, true);
assert.equal(push.track.enabled, true);
push.onBlur();
assert.equal(push.track.enabled, false, 'blur closes a gate held open by the key');

// The published track must not be the measured one. Gating the track the meter reads
// silences the meter too: the level drops to zero the moment the gate closes, so it
// never rises above the threshold again and the microphone stays shut for the whole
// call. This was real, and only showed up with an actual audio graph in the browser.
const captured = { enabled: true, clone: () => ({ enabled: true, applyConstraints: async () => {} }) };

// Node exposes `navigator` as a getter-only global, so it has to be redefined.
Object.defineProperty(globalThis, 'navigator', {
    value: { mediaDevices: { getUserMedia: async () => ({ getAudioTracks: () => [captured] }) } },
    configurable: true,
});
const node = { connect: () => node, port: { close() {} } };

globalThis.AudioContext = class {
    constructor() { this.sampleRate = 48000; this.destination = node; this.audioWorklet = { addModule: async () => {} }; }
    async resume() {}
    createMediaStreamSource() { return node; }
    createGain() { return { gain: {}, connect: () => node }; }
};
globalThis.AudioWorkletNode = class { constructor() { return node; } };
globalThis.Blob = class {};
globalThis.URL = { createObjectURL: () => 'blob:meter', revokeObjectURL() {} };

const opened = new MicrophoneGate();
const published = await opened.open();

assert.equal(opened.source, captured, 'the meter listens to the captured track');
assert.notEqual(published, captured, 'what is published is a clone, never the measured track');

published.enabled = false;
assert.equal(captured.enabled, true, 'closing the gate must not silence the meter');

console.log('MicrophoneGate: ok');
