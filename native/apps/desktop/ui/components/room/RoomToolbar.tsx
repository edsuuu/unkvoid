import { Elapsed } from '../common/Elapsed.tsx';
import { Icon } from '../common/Icon.tsx';
import { ChannelsMenu } from '../hub/ChannelsMenu.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { PeopleMenu } from './PeopleMenu.tsx';
import { ShareButton } from './ShareButton.tsx';

type RoomToolbarProps = {
    mode: 'code' | 'voice';
    chatOpen?: boolean;
    onToggleChat?: (() => void) | null;
};

export function RoomToolbar({ mode, chatOpen = false, onToggleChat = null }: RoomToolbarProps) {
    const app = useApp();
    const voice = app.hub.voice;
    const { room } = useStore(app.store);
    const { user } = useStore(app.hub.store);
    const { connectedAt, ping } = useStore(app.media.store);
    const { line } = useStore(app.sharing.store);
    const voiceState = useStore(voice.store);
    const inVoice = mode === 'voice';
    const speakless = ! voiceState.can.includes('speak');
    const micTitle = voiceState.serverMuted
        ? 'Um moderador mutou você neste servidor'
        : speakless ? 'Você não tem permissão para falar neste canal' : voiceState.muted ? 'Ativar o microfone' : 'Mutar o microfone';

    return (
        <div className="glass relative z-30 flex flex-none flex-wrap items-center justify-between gap-3 rounded-2xl px-3 py-2">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
                {inVoice
                    ? (
                        <>
                            <button className="btn-icon btn-icon-on size-[30px] rounded-[9px]" type="button" title="Voltar para os canais do servidor" onClick={() => app.hub.setFocusedRoom(false)}>
                                <Icon name="grid" size={14} />
                            </button>
                            <ChannelsMenu onOpenText={chatOpen ? null : onToggleChat} />
                        </>
                    )
                    : (
                        <>
                            {user && (
                                <button className="btn-icon size-[30px] rounded-[9px]" type="button" title="Sair da sala e ir para a Home" onClick={() => void app.goHome()}>
                                    <Icon name="home" size={14} />
                                </button>
                            )}
                            <span className="label-mono">Sala</span>
                            <button className="code-chip flex cursor-pointer items-center gap-1.5 text-[12px] hover:text-ink-strong" type="button" title="Copiar o código para mandar a alguém" onClick={() => void app.copy(room ?? '', 'Código copiado')}>
                                {room}
                                <Icon name="copy" size={12} />
                            </button>
                        </>
                    )}

                <PeopleMenu />

                <span className="flex flex-none items-center gap-2 rounded-full border border-line-strong bg-row px-3 py-1.5 font-mono text-[11px] text-ink-icon" title="Tempo na sala">
                    <span className="relative size-[7px]">
                        <span className="absolute inset-0 animate-ping-soft rounded-full bg-online" />
                        <span className="absolute inset-0 rounded-full bg-online" />
                    </span>
                    <Elapsed since={connectedAt} />
                </span>

                <span className="hidden font-mono text-[10.5px] whitespace-nowrap text-ink-dim tabular-nums sm:inline" title="Ida e volta até o servidor de mídia">
                    {ping === null ? '-- ms' : `${ping} ms`}
                    {line?.starting && ' · transmitindo…'}
                    {line && ! line.starting && ` · ${line.fps} fps · ${line.mbps.toFixed(1)} Mb/s · ${line.dropped} perdidos`}
                </span>
            </div>

            <div className="flex flex-none items-center gap-[7px]">
                {inVoice && voiceState.can.includes('video') && (
                    <button className={`btn-icon ${voiceState.cameraOn ? 'btn-icon-on' : ''}`} type="button" title={voiceState.cameraOn ? 'Desligar a câmera' : 'Ligar a câmera'} onClick={() => void voice.toggleCamera()}>
                        <Icon name={voiceState.cameraOn ? 'camera' : 'cameraOff'} size={17} />
                    </button>
                )}

                {inVoice && (
                    <button className={`btn-icon ${voiceState.muted || voiceState.serverMuted || speakless ? 'btn-icon-off' : ''}`} type="button" title={micTitle} disabled={voiceState.serverMuted || speakless} onClick={() => void voice.toggleMute()}>
                        <Icon name={voiceState.muted || voiceState.serverMuted || speakless ? 'micOff' : 'mic'} size={16} />
                    </button>
                )}

                {inVoice && (
                    <button className={`btn-icon ${voiceState.deafened ? 'btn-icon-off' : ''}`} type="button" title={voiceState.deafened ? 'Voltar a ouvir' : 'Ensurdecer: não ouvir ninguém'} onClick={() => void voice.toggleDeafen()}>
                        <Icon name={voiceState.deafened ? 'headphonesOff' : 'headphones'} size={16} />
                    </button>
                )}

                {inVoice && onToggleChat && (
                    <button className={`btn-icon ${chatOpen ? 'btn-icon-on' : ''}`} type="button" title={chatOpen ? 'Fechar o chat' : 'Ver o chat'} onClick={onToggleChat}>
                        <Icon name="chat" size={16} />
                    </button>
                )}

                {(! inVoice || voiceState.can.includes('stream')) && <ShareButton />}

                <button
                    className="btn-icon border-transparent bg-danger text-ink-strong hover:border-transparent hover:brightness-110"
                    type="button"
                    title={inVoice ? 'Sair da voz' : 'Sair da sala'}
                    onClick={() => void (inVoice ? voice.leave() : app.leave())}
                >
                    <Icon name="phoneOff" size={17} />
                </button>
            </div>
        </div>
    );
}
