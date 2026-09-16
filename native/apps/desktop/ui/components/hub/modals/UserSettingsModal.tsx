import { useEffect, useState } from 'react';

import { Failure } from '../../../core/Failure.ts';
import { Mic } from '../../../core/Mic.ts';
import { Platform } from '../../../core/Platform.ts';
import type { InputMode } from '../../../core/Voice.ts';
import { Avatar } from '../../common/Avatar.tsx';
import { Icon } from '../../common/Icon.tsx';
import { KeybindField } from '../../common/KeybindField.tsx';
import { Modal } from '../../common/Modal.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

type ToggleKey = 'noiseSuppression' | 'muteOnJoin';

const MODES: [InputMode, string, string][] = [
    ['voice', 'Detecção de voz', 'abre o microfone quando você fala'],
    ['ptt', 'Apertar para falar', 'só abre enquanto a tecla estiver pressionada'],
    ['open', 'Sempre aberto', 'o microfone fica ligado o tempo todo'],
];

export function UserSettingsModal() {
    const app = useApp();
    const hub = app.hub;
    const voice = hub.voice;
    const { user } = useStore(hub.store);
    const { preferences, talkKeyRefused } = useStore(voice.store);
    const live = useStore(voice.mic.store);
    const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
    const [level, setLevel] = useState(0);
    const [meterError, setMeterError] = useState('');
    const native = Platform.isLinux();

    useEffect(() => {
        if (native || voice.micTrack || ! navigator.mediaDevices?.getUserMedia) {
            return undefined;
        }

        let stream: MediaStream | null = null;
        let context: AudioContext | null = null;
        let frame = 0;
        let cancelled = false;

        const start = async () => {
            try {
                stream = await navigator.mediaDevices.getUserMedia({
                    audio: { deviceId: preferences.microphone ? { ideal: preferences.microphone } : undefined, noiseSuppression: preferences.noiseSuppression, echoCancellation: true },
                });

                if (cancelled) {
                    stream.getTracks().forEach(track => track.stop());

                    return;
                }

                setDevices((await navigator.mediaDevices.enumerateDevices()).filter(device => device.kind === 'audioinput'));

                if (cancelled) {
                    return;
                }

                setMeterError('');

                const meterContext = new AudioContext();
                const analyser = meterContext.createAnalyser();
                const samples = new Float32Array(Mic.FFT_SIZE);
                let shown = 0;

                context = meterContext;
                analyser.fftSize = Mic.FFT_SIZE;
                meterContext.createMediaStreamSource(stream).connect(analyser);

                const tick = () => {
                    analyser.getFloatTimeDomainData(samples);

                    const next = Mic.levelOf(samples);

                    if (Math.abs(next - shown) > 2) {
                        shown = next;
                        setLevel(next);
                    }

                    frame = requestAnimationFrame(tick);
                };

                tick();
            } catch (failure) {
                app.log('voice.meter.error', { message: Failure.message(failure) });
                setMeterError('Sem acesso ao microfone. Autorize o app nas configurações do sistema.');
            }
        };

        void start();

        return () => {
            cancelled = true;
            cancelAnimationFrame(frame);
            stream?.getTracks().forEach(track => track.stop());
            void context?.close();
        };
    }, [native, voice.micTrack, preferences.microphone, preferences.noiseSuppression, app]);

    if (! user) {
        return null;
    }

    const shown = voice.micTrack ? live.level : level;
    const chosen = Object.values(preferences.keybinds).filter(key => key.trim() !== '');
    const repeated = new Set(chosen).size !== chosen.length;

    const toggle = (key: ToggleKey, label: string) => (
        <button
            className={`cursor-pointer rounded-[10px] border px-3.5 py-2 text-[12.5px] transition ${preferences[key] ? 'border-brand/50 bg-brand/20 text-ink-strong' : 'border-line-strong bg-row text-ink-icon hover:border-brand/40'}`}
            type="button"
            aria-pressed={preferences[key]}
            onClick={() => void voice.setPreference(key, ! preferences[key])}
        >
            {preferences[key] && <Icon name="check" size={12} className="mr-1.5 inline" />}
            {label}
        </button>
    );

    return (
        <Modal
            title={user.name}
            subtitle="Online"
            leading={<Avatar name={user.name} size={40} mine />}
            width={460}
            onClose={() => hub.closeModal()}
            footer={(
                <>
                    <button className="btn-quiet" type="button" onClick={() => { hub.closeModal(); void hub.logout(); }}>Sair da conta</button>
                    <button className="btn-ghost" type="button" onClick={() => { hub.closeModal(); void app.openLogs(); }}>Logs</button>
                    <span className="flex-1" />
                    <button className="btn-primary px-4 py-2 text-[13px]" type="button" onClick={() => hub.closeModal()}>Pronto</button>
                </>
            )}
        >
            <p className="label-mono mb-2">Microfone</p>
            {native
                ? <p className="text-[12.5px] text-ink-soft">No Linux o microfone é o padrão do sistema (PulseAudio), escolhido nas configurações de som.</p>
                : (
                    <>
                        <select className="field w-full cursor-pointer text-[13px]" value={preferences.microphone} onChange={event => void voice.setPreference('microphone', event.target.value)}>
                            <option value="">Microfone padrão</option>
                            {devices.filter(device => device.deviceId !== 'default').map((device, index) => (
                                <option key={device.deviceId} value={device.deviceId}>{device.label || `Microfone ${index + 1}`}</option>
                            ))}
                        </select>
                        <div className="mt-2.5 flex items-center gap-2.5">
                            <span className="text-[12.5px] text-ink-soft">Entrada</span>
                            <span className="relative h-[7px] flex-1 overflow-hidden rounded-full bg-white/[0.08]">
                                <span
                                    className={`absolute inset-y-0 left-0 rounded-full transition-[width] duration-75 ${shown >= preferences.sensitivity ? 'bg-gradient-to-r from-brand to-online' : 'bg-white/25'}`}
                                    style={{ width: `${shown}%` }}
                                />
                                {preferences.inputMode === 'voice' && <span className="absolute inset-y-0 w-px bg-danger" style={{ left: `${preferences.sensitivity}%` }} />}
                            </span>
                        </div>
                        {meterError && <p className="mt-2 text-[12px] text-danger">{meterError}</p>}
                    </>
                )}

            <div className="mt-4 flex flex-wrap gap-2">
                {! native && toggle('noiseSuppression', 'Supressão de ruído')}
                {toggle('muteOnJoin', 'Silenciar ao entrar')}
            </div>

            <p className="label-mono mt-6 mb-2">Como o microfone abre</p>
            <div className="flex flex-col gap-1.5">
                {MODES.map(([mode, label, hint]) => (
                    <button
                        key={mode}
                        className={`row-item w-full cursor-pointer text-left ${preferences.inputMode === mode ? 'row-item-on' : ''}`}
                        type="button"
                        onClick={() => void voice.setPreference('inputMode', mode)}
                    >
                        <span className={`size-[9px] flex-none rounded-full ${preferences.inputMode === mode ? 'bg-brand' : 'bg-white/20'}`} />
                        <span className="min-w-0 flex-1">
                            <span className="block text-[12.5px]">{label}</span>
                            <span className="block text-[11px] text-ink-dim">{hint}</span>
                        </span>
                    </button>
                ))}
            </div>

            {preferences.inputMode === 'voice' && (
                <label className="mt-3 flex flex-col gap-1 text-[12px] text-ink-soft">
                    Sensibilidade — abre acima de {preferences.sensitivity}%
                    <input
                        className="accent-brand"
                        type="range"
                        min="5"
                        max="90"
                        value={preferences.sensitivity}
                        onChange={event => void voice.setPreference('sensitivity', Number(event.target.value))}
                    />
                </label>
            )}

            {preferences.inputMode === 'ptt' && preferences.keybinds.talk.trim() === '' && (
                <p className="mt-2 text-[12px] text-danger">Escolha a tecla de apertar para falar aqui embaixo — sem ela o microfone fica sempre aberto.</p>
            )}

            {preferences.inputMode === 'ptt' && preferences.keybinds.talk.trim() !== '' && talkKeyRefused && (
                <p className="mt-2 text-[12px] text-danger">O sistema recusou essa tecla (outro programa já usa): o microfone fica sempre aberto até você escolher outra.</p>
            )}

            {native && preferences.inputMode === 'voice' && (
                <p className="mt-2 text-[12px] text-ink-dim">No Linux o microfone é lido fora da janela, então a detecção de voz não mede o nível: use apertar para falar.</p>
            )}

            <p className="label-mono mt-6 mb-2">Teclas</p>
            <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 text-[12.5px] text-ink-soft">Mutar o microfone</span>
                    <KeybindField value={preferences.keybinds.mute} onChange={mute => void voice.setPreference('keybinds', { ...preferences.keybinds, mute })} />
                </div>
                <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 text-[12.5px] text-ink-soft">Mutar o áudio de todos</span>
                    <KeybindField value={preferences.keybinds.deafen} onChange={deafen => void voice.setPreference('keybinds', { ...preferences.keybinds, deafen })} />
                </div>
                <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 text-[12.5px] text-ink-soft">Apertar para falar</span>
                    <KeybindField value={preferences.keybinds.talk} bare onChange={talk => void voice.setPreference('keybinds', { ...preferences.keybinds, talk })} />
                </div>
            </div>
            <p className="mt-2 text-[11.5px] text-ink-dim">As teclas valem com o jogo na frente. Esc dentro do campo apaga a tecla.</p>

            {repeated && <p className="mt-1 text-[11.5px] text-danger">Duas ações com a mesma tecla: o sistema só aceita a primeira.</p>}
        </Modal>
    );
}
