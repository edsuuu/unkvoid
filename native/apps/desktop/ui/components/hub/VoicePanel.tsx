import { useState } from 'react';

import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
import { Spinner } from '../common/Spinner.tsx';
import { ShareButton } from '../room/ShareButton.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { ClipButton } from './ClipButton.tsx';

const MENU_ITEM = 'flex w-full cursor-pointer items-center gap-2.5 rounded-[9px] px-2.5 py-2 text-left text-[12.5px] text-ink-icon hover:bg-row hover:text-ink-strong';

export function VoicePanel() {
    const app = useApp();
    const hub = app.hub;
    const voice = hub.voice;
    const { user } = useStore(hub.store);
    const voiceState = useStore(voice.store);
    const { reconnecting, ping } = useStore(app.media.store);
    const [menuOpen, setMenuOpen] = useState(false);
    const choose = (work: () => unknown) => {
        setMenuOpen(false);
        void work();
    };
    const voiceChannel = voiceState.channel;
    const speakless = ! voiceState.can.includes('speak');
    const micOff = voiceState.muted || voiceState.serverMuted || speakless;
    const status = ! voiceChannel ? 'Online' : voiceState.micProblem !== '' ? 'Sem microfone' : voiceState.deafened ? 'Surdo' : voiceState.serverMuted ? 'Mutado pelo servidor' : micOff ? 'Mudo' : voiceState.speaking ? 'Falando' : 'Microfone aberto';

    return (
        <div className="glass relative z-30 flex flex-none flex-col gap-2.5 p-3">
            {voiceChannel && (
                <div className="animate-rise">
                    <div className="flex items-center gap-2.5">
                        <span
                            className={voiceState.joining || reconnecting ? 'text-ink-dim' : ping !== null && ping > 150 ? 'text-periwinkle' : 'text-online'}
                            title={voiceState.joining ? 'Conectando…' : reconnecting ? 'Reconectando…' : ping === null ? 'Medindo a ida e volta até o servidor de mídia' : `${ping} ms até o servidor de mídia`}
                        >
                            {voiceState.joining || reconnecting ? <Spinner size={14} /> : <Icon name="signal" size={15} />}
                        </span>
                        <span className="min-w-0 flex-1">
                            <span className={`block text-[13px] font-semibold ${reconnecting ? 'text-danger' : voiceState.joining ? 'text-ink-icon' : 'text-online'}`}>
                                {voiceState.joining ? 'Conectando…' : reconnecting ? 'Reconectando…' : 'Voz conectada'}
                            </span>
                            <span className="mt-0.5 block truncate font-mono text-[10.5px] text-ink-dim">{voiceChannel.name}</span>
                        </span>
                        <button className="btn-icon size-7 rounded-[9px] text-periwinkle hover:border-danger/50 hover:text-danger" type="button" title="Desconectar da voz" onClick={() => void voice.leave()}>
                            <Icon name="phoneOff" size={14} />
                        </button>
                    </div>

                    <div className="mt-2.5 grid auto-cols-fr grid-flow-col gap-1.5">
                        <button className={`btn-icon h-8 w-full rounded-[9px] ${voiceState.cameraOn ? 'btn-icon-on' : ''}`} type="button" title={voiceState.cameraOn ? 'Desligar a câmera' : 'Ligar a câmera'} disabled={voiceState.joining || ! voiceState.can.includes('video')} onClick={() => void voice.toggleCamera()}>
                            <Icon name={voiceState.cameraOn ? 'camera' : 'cameraOff'} size={16} />
                        </button>
                        <ShareButton wide disabled={voiceState.joining || ! voiceState.can.includes('stream')} />
                        <ClipButton wide />
                    </div>
                </div>
            )}

            <div className={`flex items-center gap-2.5 ${voiceChannel ? 'border-t border-line pt-2.5' : ''}`}>
                <Avatar name={user?.name} size={30} mine />
                <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12.5px] font-semibold">{user?.name}</span>
                    <span className="mt-px block truncate text-[10.5px] text-ink-dim">{status}</span>
                </span>
                <button
                    className={`flex size-[26px] cursor-pointer items-center justify-center rounded-lg transition hover:bg-row disabled:cursor-not-allowed disabled:opacity-40 ${voiceChannel && micOff ? 'text-danger' : voiceState.speaking ? 'text-online ring-1 ring-online/60' : 'text-ink-icon'}`}
                    type="button"
                    title={voiceState.micProblem !== '' ? voiceState.micProblem : voiceState.serverMuted ? 'Um moderador mutou você neste servidor' : speakless && voiceChannel ? 'Você não tem permissão para falar neste canal' : micOff ? 'Ativar o microfone' : 'Mutar o microfone'}
                    disabled={! voiceChannel || voiceState.serverMuted || speakless}
                    onClick={() => void voice.toggleMute()}
                >
                    <Icon name={voiceChannel && micOff ? 'micOff' : 'mic'} size={15} />
                </button>
                <button className={`flex size-[26px] cursor-pointer items-center justify-center rounded-lg transition hover:bg-row disabled:cursor-not-allowed disabled:opacity-40 ${voiceState.deafened ? 'text-danger' : 'text-ink-icon'}`} type="button" title={voiceState.deafened ? 'Voltar a ouvir' : 'Ensurdecer: não ouvir ninguém'} disabled={! voiceChannel} onClick={() => void voice.toggleDeafen()}>
                    <Icon name={voiceState.deafened ? 'headphonesOff' : 'headphones'} size={15} />
                </button>
                <span className="relative">
                    <button
                        className={`flex size-[26px] cursor-pointer items-center justify-center rounded-lg transition hover:bg-row ${menuOpen ? 'bg-row text-ink-strong' : 'text-ink-icon'}`}
                        type="button"
                        title="Menu"
                        onClick={() => setMenuOpen(open => ! open)}
                    >
                        <Icon name="gear" size={15} />
                    </button>

                    <Popover open={menuOpen} onClose={() => setMenuOpen(false)} className="bottom-9 right-0 w-56">
                        <div className="mb-1 border-b border-line px-2.5 pt-1 pb-2">
                            <p className="truncate text-[13px] font-semibold">{user?.name ?? 'Sem conta'}</p>
                            <p className="label-mono mt-0.5">{user ? 'conta conectada' : 'usando sem login'}</p>
                        </div>
                        {user && <button className={MENU_ITEM} type="button" onClick={() => choose(() => hub.openModal({ type: 'user' }))}><Icon name="gear" size={15} />Configurações da conta</button>}
                        <button className={MENU_ITEM} type="button" onClick={() => choose(() => app.setTab('clips'))}><Icon name="scissors" size={15} />Clipes</button>
                        <button className={MENU_ITEM} type="button" onClick={() => choose(() => app.openLogs())}><Icon name="logs" size={15} />Logs</button>
                        {user && <button className={`${MENU_ITEM} text-periwinkle`} type="button" onClick={() => choose(() => hub.logout())}><Icon name="logout" size={15} />Sair da conta</button>}
                    </Popover>
                </span>
            </div>
        </div>
    );
}
