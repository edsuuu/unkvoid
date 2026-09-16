import { Permissions } from '../../core/Permissions.ts';
import { Icon } from '../common/Icon.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { VoiceChannelItem } from './VoiceChannelItem.tsx';
import { VoicePanel } from './VoicePanel.tsx';

export function ChannelColumn() {
    const hub = useApp().hub;
    const { tree, channel, stageOpen } = useStore(hub.store);

    if (! tree) {
        return null;
    }

    const channels = [...tree.channels].sort((left, right) => left.position - right.position);
    const manage = hub.can(Permissions.MANAGE_CHANNELS);
    const textChannels = channels.filter(item => item.type === 'text');
    const voiceChannels = channels.filter(item => item.type === 'voice');

    return (
        <aside className="flex w-[300px] flex-none flex-col gap-3">
            <div className="glass flex items-center gap-2.5 px-4 py-3.5">
                <h5 className="m-0 min-w-0 flex-1 truncate text-[17px] font-semibold tracking-tight">{tree.name}</h5>
                <button className="btn-icon size-7 rounded-[9px]" type="button" title="Configurações do servidor" onClick={() => hub.openModal({ type: 'settings' })}>
                    <Icon name="dots" size={15} />
                </button>
            </div>

            <div className="glass scroll-thin flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
                <div>
                    <div className="flex items-center gap-2">
                        <span className="label-mono flex-1">Canais de texto</span>
                        {manage && (
                            <button className="btn-icon size-5 rounded-[7px]" type="button" title="Criar canal de texto" onClick={() => hub.openModal({ type: 'channel', channel: null, channelType: 'text' })}>
                                <Icon name="plus" size={12} />
                            </button>
                        )}
                    </div>
                    <div className="mt-2 flex flex-col gap-1.5">
                        {textChannels.map(item => {
                            const active = item.id === channel?.id && ! stageOpen;

                            return (
                                <button key={item.id} className={`row-item w-full cursor-pointer text-left ${active ? 'row-item-on' : ''}`} type="button" onClick={() => void hub.attempt(() => hub.openChannel(item))}>
                                    <span className={`font-mono text-[11px] ${active ? 'text-lilac-2' : 'text-ink-dim'}`}>#</span>
                                    <span className={`min-w-0 flex-1 truncate text-[13px] ${active ? 'font-medium text-ink-strong' : 'text-ink-icon'}`}>{item.name}</span>
                                </button>
                            );
                        })}
                        {textChannels.length === 0 && (manage
                            ? <button className="btn-ghost w-full py-2 text-[12.5px]" type="button" onClick={() => hub.openModal({ type: 'channel', channel: null, channelType: 'text' })}>Criar canal de texto</button>
                            : <p className="text-[12px] text-ink-dim">Nenhum canal de texto visível.</p>)}
                    </div>
                </div>

                <div>
                    <div className="flex items-center gap-2">
                        <span className="label-mono flex-1">Canais de voz</span>
                        {manage && (
                            <button className="btn-icon size-5 rounded-[7px]" type="button" title="Criar canal de voz" onClick={() => hub.openModal({ type: 'channel', channel: null, channelType: 'voice' })}>
                                <Icon name="plus" size={12} />
                            </button>
                        )}
                    </div>
                    <div className="mt-2 flex flex-col gap-1.5">
                        {voiceChannels.map(item => <VoiceChannelItem key={item.id} channel={item} people={tree.voice?.[item.id] ?? []} />)}
                        {voiceChannels.length === 0 && (manage
                            ? <button className="btn-ghost w-full py-2 text-[12.5px]" type="button" onClick={() => hub.openModal({ type: 'channel', channel: null, channelType: 'voice' })}>Criar canal de voz</button>
                            : <p className="text-[12px] text-ink-dim">Nenhum canal de voz visível.</p>)}
                    </div>
                </div>
            </div>

            <VoicePanel />
        </aside>
    );
}
