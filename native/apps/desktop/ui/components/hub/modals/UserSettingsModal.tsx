import { useEffect, useState } from 'react';

import { Failure } from '../../../core/Failure.ts';
import { Platform } from '../../../core/Platform.ts';
import { Avatar } from '../../common/Avatar.tsx';
import { Icon } from '../../common/Icon.tsx';
import { Modal } from '../../common/Modal.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

type ToggleKey = 'noiseSuppression' | 'muteOnJoin';

export function UserSettingsModal() {
    const app = useApp();
    const hub = app.hub;
    const voice = hub.voice;
    const { user } = useStore(hub.store);
    const { preferences } = useStore(voice.store);
    const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
    const [level, setLevel] = useState(0);
    const [meterError, setMeterError] = useState('');
    const native = Platform.isLinux();

    useEffect(() => {
        if (native || ! navigator.mediaDevices?.getUserMedia) {
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
                const samples = new Uint8Array(512);
                let shown = 0;

                context = meterContext;
                analyser.fftSize = 512;
                meterContext.createMediaStreamSource(stream).connect(analyser);

                const tick = () => {
                    analyser.getByteTimeDomainData(samples);

                    let sum = 0;

                    for (const sample of samples) {
                        sum += ((sample - 128) / 128) ** 2;
                    }

                    const next = Math.min(1, Math.sqrt(sum / samples.length) * 4);

                    if (Math.abs(next - shown) > 0.03) {
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
    }, [native, preferences.microphone, preferences.noiseSuppression, app]);

    if (! user) {
        return null;
    }

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
                            <span className="relative h-[5px] flex-1 overflow-hidden rounded-full bg-white/[0.08]">
                                <span className="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-brand to-online transition-[width] duration-75" style={{ width: `${Math.round(level * 100)}%` }} />
                            </span>
                        </div>
                        {meterError && <p className="mt-2 text-[12px] text-danger">{meterError}</p>}
                    </>
                )}

            <div className="mt-4 flex flex-wrap gap-2">
                {! native && toggle('noiseSuppression', 'Supressão de ruído')}
                {toggle('muteOnJoin', 'Silenciar ao entrar')}
            </div>
        </Modal>
    );
}
