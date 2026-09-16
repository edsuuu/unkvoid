import { useState } from 'react';

import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function PeopleMenu() {
    const app = useApp();
    const media = app.media;
    const { peers, connecting } = useStore(media.store);
    const { channel: voiceChannel } = useStore(app.hub.voice.store);
    const { tree } = useStore(app.hub.store);
    const [open, setOpen] = useState(false);
    const present = peers.filter(peer => ! peer.reconnecting);

    return (
        <span className="relative flex-none">
            <button
                className="flex cursor-pointer items-center gap-2.5 rounded-full border border-line-strong bg-row py-[5px] pr-3 pl-1.5 text-[12px] text-ink-icon transition hover:border-brand/50"
                type="button"
                title="Quem está na sala"
                onClick={() => setOpen(value => ! value)}
            >
                <span className="flex items-center gap-[5px]">
                    {peers.slice(0, 3).map(peer => (
                        <span key={peer.peerId} className={`rounded-full ${peer.self ? '' : 'ring-1 ring-white/[0.12]'}`}>
                            <Avatar name={peer.name} size={26} mine={peer.self} />
                        </span>
                    ))}
                </span>
                <span className="font-mono text-[10.5px] text-ink-dim">{connecting ? 'conectando…' : present.length || 1}</span>
                <Icon name="chevronDown" size={12} />
            </button>

            <Popover open={open} onClose={() => setOpen(false)} className="top-10 left-0 w-80">
                <div className="flex items-center px-2 py-1.5">
                    <span className="label-mono flex-1">Na sala</span>
                    <button className="flex cursor-pointer items-center gap-1 text-[11px] text-ink-dim hover:text-ink-strong" type="button" title="Procurar de novo quem está transmitindo" onClick={() => void media.refreshWatch()}>
                        <Icon name="refresh" size={12} />
                        Atualizar
                    </button>
                </div>

                {peers.length === 0 && <p className="px-2 py-2 text-[12.5px] text-ink-soft">Nenhuma pessoa conectada.</p>}

                {peers.map(peer => {
                    const micPaused = peer.producers?.some(producer => producer.source === 'mic' && producer.paused);
                    const info = peer.reconnecting ? 'parado' : `${peer.latency ?? '--'} ms`;
                    const member = voiceChannel && ! peer.self && peer.userId?.startsWith('user:')
                        ? tree?.members.find(item => `user:${item.user_id}` === peer.userId)
                        : null;

                    return (
                        <div key={peer.peerId} className={`flex items-center gap-2.5 rounded-[10px] p-2 ${peer.self ? 'bg-online/10' : ''}`}>
                            <Avatar name={peer.name} size={24} mine={peer.self} />
                            <span className={`min-w-0 flex-1 truncate text-[12.5px] ${peer.reconnecting ? 'text-ink-dim' : 'text-ink-body'}`}>{peer.name}</span>
                            {micPaused && <span className="text-danger" title="Microfone mutado"><Icon name="micOff" size={13} /></span>}
                            {peer.sharing && <span className="live-badge">AO VIVO</span>}
                            <span className="font-mono text-[10px] text-ink-dim">{peer.self ? 'você' : info}</span>
                            {peer.missing && (
                                <button className="btn-ghost px-2 py-1 text-[11px]" type="button" onClick={() => void media.watchPeer(peer.peerId)}>Assistir</button>
                            )}
                            {member && (
                                <button className="btn-icon size-7 rounded-[8px]" type="button" title="Banir, expulsar, desconectar e mais" onClick={event => { setOpen(false); app.hub.openMemberMenu(member, event.clientX, event.clientY); }}>
                                    <Icon name="dots" size={14} />
                                </button>
                            )}
                            {peer.reconnecting && ! peer.self && (
                                <button className="btn-danger px-2 py-1 text-[11px]" type="button" onClick={() => { setOpen(false); void media.removeStoppedPeer(peer.peerId); }}>Remover</button>
                            )}
                        </div>
                    );
                })}
            </Popover>
        </span>
    );
}
