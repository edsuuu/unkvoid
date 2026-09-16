import { Icon } from '../common/Icon.tsx';
import { Stage } from '../room/Stage.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { ChannelColumn } from './ChannelColumn.tsx';
import { ChatPanel } from './ChatPanel.tsx';
import { MemberList } from './MemberList.tsx';

export function ServerView() {
    const hub = useApp().hub;
    const { tree, stageOpen, inviteBanner, membersOpen } = useStore(hub.store);
    const voiceState = useStore(hub.voice.store);
    const stageChannel = stageOpen ? voiceState.channel : null;

    return (
        <div className="flex min-w-0 flex-1 gap-3">
            <ChannelColumn />

            <div className="flex min-w-0 flex-1 flex-col gap-3">
                {inviteBanner && (
                    <div className="glass flex animate-rise items-center gap-3 rounded-2xl px-4 py-2.5 text-[13px]">
                        <span className="text-ink-soft">Convite da sala</span>
                        <code className="code-chip">{tree?.invite_code}</code>
                        <button className="btn-ghost flex items-center gap-1.5 px-2.5 py-1.5 text-[12px]" type="button" onClick={() => void hub.copyInvite()}>
                            <Icon name="copy" size={12} />
                            Copiar
                        </button>
                        <span className="flex-1" />
                        <button className="flex size-7 flex-none cursor-pointer items-center justify-center rounded-md text-ink-dim hover:bg-row hover:text-ink-strong" type="button" title="Fechar" onClick={() => hub.closeInviteBanner()}>
                            <Icon name="close" size={14} />
                        </button>
                    </div>
                )}

                {stageChannel
                    ? (
                        <section className="glass flex min-h-0 flex-1 animate-fade-in flex-col gap-3 border-danger/25 p-4">
                            <div className="flex items-center gap-2">
                                <span className="text-online"><Icon name="speaker" size={15} /></span>
                                <span className="min-w-0 truncate text-[14px] font-semibold">{stageChannel.name}</span>
                                <span className="flex-1" />
                                <button className="btn-ghost flex items-center gap-1.5 px-2.5 py-1.5 text-[12px]" type="button" title="Mudar visual para focado" onClick={() => hub.setFocusedRoom(true)}>
                                    <Icon name="focus" size={13} />
                                    Sala focada
                                </button>
                                <button className="btn-ghost px-2.5 py-1.5 text-[12px]" type="button" onClick={() => hub.showStage(false)}>Voltar ao chat</button>
                            </div>
                            <Stage canShare={voiceState.can.includes('stream')} compact />
                        </section>
                    )
                    : <ChatPanel />}
            </div>

            {membersOpen && ! stageChannel && <MemberList />}
        </div>
    );
}
