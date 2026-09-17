import { useState } from 'react';

import type { Channel, ChannelType, Overwrite } from '../../../core/Models.ts';
import { Permissions, type PermissionName } from '../../../core/Permissions.ts';
import { ServerSettings, type ShownOverwriteTarget } from '../../../core/ServerSettings.ts';
import { Modal } from '../../common/Modal.tsx';
import { useApp } from '../../useApp.ts';

const CELL_LOOK = {
    allow: { className: 'bg-online text-back', text: '✓' },
    deny: { className: 'bg-danger text-ink-strong', text: '✕' },
    inherit: { className: 'bg-row text-ink-dim', text: '—' },
};

export function ChannelModal({ channel, channelType }: { channel: Channel | null; channelType: ChannelType }) {
    const hub = useApp().hub;
    const settings = hub.settings;
    const [name, setName] = useState(channel?.name ?? '');
    const [type, setType] = useState<ChannelType>(channel?.type ?? channelType ?? 'text');
    const [topic, setTopic] = useState(channel?.topic ?? '');
    const [limit, setLimit] = useState<string | number>(channel?.user_limit ?? '');
    const [overwrites, setOverwrites] = useState<Overwrite[]>(channel?.overwrites ?? []);
    const canHide = Boolean(channel) && hub.can(Permissions.MANAGE_ROLES);
    const { shown, available } = canHide ? settings.overwriteTargets(overwrites) : { shown: [], available: [] };

    const addTarget = (value: string) => {
        const [targetType, id] = value.split(':');
        const target = available.find(item => item.type === targetType && String(item.id) === id);

        if (target) {
            setOverwrites(current => [...current, { target_type: target.type, target_id: target.id, allow: 0, deny: 0 }]);
        }
    };

    const cycle = async (target: ShownOverwriteTarget, flag: PermissionName) => {
        if (! channel) {
            return;
        }

        const next = await settings.cycleOverwrite(channel, overwrites, target, flag);

        if (next) {
            setOverwrites(next);
        }
    };

    return (
        <Modal
            title={channel ? `Canal: ${channel.name}` : type === 'voice' ? 'Novo canal de voz' : 'Novo canal de texto'}
            width={canHide ? 640 : 480}
            onClose={() => hub.closeModal()}
            footer={(
                <>
                    {channel && <button className="btn-danger" type="button" onClick={() => void settings.deleteChannel(channel)}>Apagar canal</button>}
                    <span className="flex-1" />
                    <button className="btn-ghost" type="button" onClick={() => hub.closeModal()}>Cancelar</button>
                    <button className="btn-primary px-4 py-2 text-[13px]" type="submit" form="channel-form">Salvar</button>
                </>
            )}
        >
            <form id="channel-form" className="flex flex-col gap-2.5" noValidate onSubmit={event => { event.preventDefault(); void settings.saveChannel(channel, { name, type, topic, limit: String(limit) }); }}>
                <div className="flex gap-2">
                    <input className="field min-w-0 flex-1" type="text" maxLength={40} value={name} onChange={event => setName(event.target.value)} placeholder="Nome" autoCapitalize="off" autoCorrect="off" spellCheck={false} autoFocus />
                    <select className="field cursor-pointer text-[13px]" value={type} disabled={Boolean(channel)} onChange={event => setType(event.target.value as ChannelType)}>
                        <option value="text">Texto</option>
                        <option value="voice">Voz</option>
                    </select>
                </div>
                <input className="field w-full" type="text" maxLength={200} value={topic} onChange={event => setTopic(event.target.value)} placeholder="Tópico (opcional)" />
                {type === 'voice' && (
                    <input className="field w-48" type="number" min="1" max="99" value={limit} onChange={event => setLimit(event.target.value)} placeholder="Limite de pessoas" title="Vazio = sem limite" />
                )}
            </form>

            {canHide && (
                <div className="mt-5">
                    <div className="flex items-center gap-2">
                        <span className="label-mono flex-1">Ocultar canal</span>
                        <select className="field cursor-pointer px-2 py-1.5 text-[12px]" value="" onChange={event => addTarget(event.target.value)}>
                            <option value="">Adicionar cargo ou membro…</option>
                            {available.map(target => (
                                <option key={`${target.type}:${target.id}`} value={`${target.type}:${target.id}`}>{target.type === 'role' ? 'cargo' : 'membro'}: {target.name}</option>
                            ))}
                        </select>
                    </div>
                    <p className="mt-1.5 text-[12px] text-ink-dim">Clique numa célula para alternar: — herda, ✓ permite, ✕ nega. Grava na hora.</p>

                    <div className="label-mono mt-3 grid grid-cols-[1fr_repeat(6,2.6rem)] gap-1 px-2">
                        <span />
                        {Permissions.OVERWRITABLE.map(flag => <span key={flag} className="text-center" title={flag}>{ServerSettings.OVERWRITE_LABELS[flag]}</span>)}
                    </div>
                    <div className="mt-1 flex flex-col gap-1">
                        {shown.map(target => (
                            <div key={`${target.type}:${target.id}`} className="row-item grid grid-cols-[1fr_repeat(6,2.6rem)] gap-1 py-1.5 text-[13px]">
                                <span className="truncate" style={{ color: target.color ?? undefined }}>{target.name}</span>
                                {Permissions.OVERWRITABLE.map(flag => {
                                    const bit = Permissions[flag];
                                    const state = (target.allow & bit) ? 'allow' : (target.deny & bit) ? 'deny' : 'inherit';

                                    return (
                                        <button key={flag} className={`cursor-pointer rounded py-0.5 text-center text-[12px] ${CELL_LOOK[state].className}`} type="button" onClick={() => void cycle(target, flag)}>
                                            {CELL_LOOK[state].text}
                                        </button>
                                    );
                                })}
                            </div>
                        ))}
                    </div>
                </div>
            )}
        </Modal>
    );
}
