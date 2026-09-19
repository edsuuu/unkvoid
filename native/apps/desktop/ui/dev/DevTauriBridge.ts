import type { TauriBridge, TauriEvent } from '../core/Tauri.ts';

type Binding = { action: string; accelerator: string };

type BridgeArgs = { source?: string; url?: string; bindings?: Binding[] };

type Listener = (event: TauriEvent<never>) => void;

type Answer = (args: BridgeArgs) => unknown;

export class DevTauriBridge {
    static install(): void {
        if (! import.meta.env.DEV || window.__TAURI__) {
            return;
        }

        const ssrc: Record<string, number> = { screen: 0x2234_5678, screenAudio: 0x2234_5679, camera: 0x2234_567a, mic: 0x2234_567b };
        const video = {
            mimeType: 'video/H264',
            payloadType: 96,
            clockRate: 90_000,
            parameters: { 'packetization-mode': 1, 'level-asymmetry-allowed': 1, 'profile-level-id': '42e01f' },
            rtcpFeedback: [{ type: 'nack' }, { type: 'nack', parameter: 'pli' }, { type: 'ccm', parameter: 'fir' }, { type: 'goog-remb' }],
        };
        const audio = { mimeType: 'audio/opus', payloadType: 111, clockRate: 48_000, channels: 2, parameters: { useinbandfec: 1, usedtx: 1 }, rtcpFeedback: [] };
        const key = btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(30))));
        const ceilingBitrate = 8_000_000;
        const stats = { active: false, captured: 0, sent: 0, sentBytes: 0, sendDropped: 0, encodeErrors: 0, sendErrors: 0, audioErrors: 0, busyUs: 0, encoder: 'gpu', targetBitrate: ceilingBitrate, lossPermille: 0 };
        const preview = (label: string) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="320" height="200"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#5a3fd6"/><stop offset="1" stop-color="#08060e"/></linearGradient></defs><rect width="320" height="200" fill="url(#g)"/><text x="160" y="106" fill="#cfc9de" font-family="monospace" font-size="14" text-anchor="middle">${label}</text></svg>`)}`;

        const listeners = new Map<string, Set<Listener>>();
        const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach(listener => listener({ payload } as TauriEvent<never>));
        const mouseButtons: Record<number, string> = { 1: 'Mouse3', 3: 'Mouse4', 4: 'Mouse5' };
        let shortcuts: Binding[] = [];
        let levelTimer: ReturnType<typeof setInterval> | null = null;

        const pressed = (event: KeyboardEvent | MouseEvent, code: string | undefined, down: boolean) => {
            for (const binding of shortcuts) {
                const parts = binding.accelerator.replace('CmdOrCtrl', 'Control').split('+');
                const held = parts.slice(0, -1).every(part => ({ Control: event.ctrlKey, Shift: event.shiftKey, Alt: event.altKey, Super: event.metaKey })[part]);

                if (parts.at(-1) === code && (held || ! down)) {
                    emit('shortcut', { action: binding.action, pressed: down });
                }
            }
        };

        window.addEventListener('keydown', event => ! event.repeat && pressed(event, event.code, true));
        window.addEventListener('keyup', event => pressed(event, event.code, false));
        window.addEventListener('mousedown', event => pressed(event, mouseButtons[event.button], true));
        window.addEventListener('mouseup', event => pressed(event, mouseButtons[event.button], false));

        const answers: Record<string, Answer> = {
            check_update: () => null,
            set_shortcuts: ({ bindings = [] }) => {
                shortcuts = bindings;

                return { registered: bindings.map(binding => binding.action), failed: [] };
            },
            start_voice: () => {
                const startedAt = Date.now();

                clearInterval(levelTimer ?? undefined);
                levelTimer = setInterval(() => {
                    const seconds = (Date.now() - startedAt) / 1000;

                    emit('voice:level', { level: seconds % 4 < 2 ? 0.08 + 0.04 * Math.sin(seconds * 9) : 0.0005 });
                }, 100);

                return null;
            },
            stop_voice: () => {
                clearInterval(levelTimer ?? undefined);
                levelTimer = null;

                return null;
            },
            set_voice_muted: () => null,
            restart: () => null,
            expand_window: () => null,
            log_line: () => null,
            log_path: () => '',
            list_displays: () => [{ id: 1, width: 2560, height: 1440 }, { id: 2, width: 1920, height: 1080 }],
            list_windows: () => [
                { id: 87, title: 'Arena Breakout', application: 'Steam' },
                { id: 91, title: 'Terminal', application: 'Terminal' },
                { id: 93, title: 'unkvoid — main', application: 'Code' },
            ],
            list_cameras: () => [],
            source_preview: ({ source = '' }) => preview(source),
            start_broadcast: () => {
                stats.active = true;

                return null;
            },
            stop_broadcast: () => {
                stats.active = false;

                return stats.captured;
            },
            broadcast_stats: () => {
                if (stats.active) {
                    const tight = Math.floor(stats.captured / 60) % 20 >= 10;

                    stats.captured += 60;
                    stats.sent += 60;
                    stats.sentBytes += tight ? 450_000 : 1_000_000;
                    stats.targetBitrate = tight ? ceilingBitrate * 0.45 : ceilingBitrate;
                    stats.lossPermille = tight ? 62 : 3;
                }

                return { ...stats };
            },
            sfu_offer: ({ source = '' }) => ({
                rtpParameters: { codecs: [['screen', 'camera'].includes(source) ? video : audio], encodings: [{ ssrc: ssrc[source] }] },
                srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: key },
            }),
            renew_sfu_key: () => null,
            use_sfu: () => null,
            stop_watch: () => null,
            watch_mute: () => null,
            google_login: () => {
                throw new Error('sem o Tauri o Google não tem como voltar para o app');
            },
        };

        const realGetUserMedia = navigator.mediaDevices?.getUserMedia?.bind(navigator.mediaDevices);

        if (realGetUserMedia) {
            navigator.mediaDevices.getUserMedia = async (constraints?: MediaStreamConstraints) => {
                try {
                    return await realGetUserMedia(constraints);
                } catch (failure) {
                    if (constraints?.video) {
                        const canvas = Object.assign(document.createElement('canvas'), { width: 640, height: 360 });
                        const context = canvas.getContext('2d')!;
                        let frame = 0;

                        setInterval(() => {
                            frame += 1;
                            context.fillStyle = `hsl(${(frame * 2) % 360} 55% 35%)`;
                            context.fillRect(0, 0, 640, 360);
                            context.fillStyle = '#ffffff';
                            context.font = '28px monospace';
                            context.fillText(`câmera de teste ${frame}`, 40, 190);
                        }, 1000 / 30);

                        return canvas.captureStream(30);
                    }

                    const audioContext = new AudioContext();
                    const oscillator = audioContext.createOscillator();
                    const gain = audioContext.createGain();
                    const destination = audioContext.createMediaStreamDestination();

                    console.warn(`microfone negado (${(failure as DOMException).name}): usando um tom de teste`);
                    gain.gain.value = 0.03;
                    oscillator.connect(gain).connect(destination);
                    oscillator.start();

                    return destination.stream;
                }
            };
        }

        const bridge: TauriBridge = {
            core: {
                invoke: async (command, args = {}) => {
                    if (! answers[command]) {
                        throw new Error(`o comando ${command} não existe na ponte de desenvolvimento`);
                    }

                    return answers[command](args as BridgeArgs);
                },
            },
            event: {
                listen: async (event, handler) => {
                    const handlers = listeners.get(event) ?? new Set<Listener>();

                    handlers.add(handler);
                    listeners.set(event, handlers);

                    return () => handlers.delete(handler);
                },
            },
        };

        window.__TAURI__ = bridge;
    }
}
