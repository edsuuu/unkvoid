import { useState } from 'react';

import type { Channel } from '../../core/Models.ts';
import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function ChannelsMenu({ onOpenText = null }: { onOpenText?: (() => void) | null }) {
    const hub = useApp().hub;
    const { tree } = useStore(hub.store);
    const { channel: voiceChannel } = useStore(hub.voice.store);
    const [open, setOpen] = useState(false);
    const channels = [...(tree?.channels ?? [])].sort((left, right) => left.position - right.position);

    const choose = (channel: Channel) => {
        setOpen(false);

        if (channel.type === 'text') {
            onOpenText?.();
            void hub.attempt(() => hub.openChannel(channel));

            return;
        }

        if (channel.id !== voiceChannel?.id) {
            void hub.attempt(() => hub.openChannel(channel));
        }
    };

    return (
        <span className="relative flex-none">
            <button className={`btn-icon w-auto gap-1 px-2.5 ${open ? 'btn-icon-on' : ''}`} type="button" title={open ? 'Fechar os canais' : 'Ver os canais do servidor'} onClick={() => setOpen(value => ! value)}>
                <Icon name="hash" size={14} />
                <Icon name="chevronDown" size={11} />
            </button>

            <Popover open={open} onClose={() => setOpen(false)} className="top-11 left-0 w-60">
                <p className="label-mono px-2 py-1.5">Canais de texto</p>
                {channels.filter(channel => channel.type === 'text').map(channel => (
                    <button key={channel.id} className="flex w-full cursor-pointer items-center gap-2.5 rounded-[10px] px-2.5 py-2 text-left text-[12.5px] text-ink-icon hover:bg-row hover:text-ink-strong" type="button" onClick={() => choose(channel)}>
                        <span className="font-mono text-[11px] text-lilac-2">#</span>
                        <span className="truncate">{channel.name}</span>
                    </button>
                ))}

                <p className="label-mono mt-1 border-t border-line px-2 pt-2.5 pb-1.5">Canais de voz</p>
                {channels.filter(channel => channel.type === 'voice').map(channel => {
                    const here = channel.id === voiceChannel?.id;
                    const count = tree?.voice?.[channel.id]?.length ?? 0;

                    return (
                        <button key={channel.id} className={`flex w-full cursor-pointer items-center gap-2.5 rounded-[10px] border px-2.5 py-2 text-left text-[12.5px] ${here ? 'border-online/30 bg-online/10 text-ink-body' : 'border-transparent text-ink-icon hover:bg-row hover:text-ink-strong'}`} type="button" onClick={() => choose(channel)}>
                            <span className={here ? 'text-online' : 'text-ink-dim'}><Icon name="speaker" size={13} /></span>
                            <span className="min-w-0 flex-1 truncate">{channel.name}</span>
                            <span className={`font-mono text-[9.5px] ${here ? 'text-online' : 'text-ink-dim'}`}>{here ? 'aqui' : count || 'vazio'}</span>
                        </button>
                    );
                })}
            </Popover>
        </span>
    );
}
