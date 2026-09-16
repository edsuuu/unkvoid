import { useEffect, useLayoutEffect, useRef, useState } from 'react';

import { Members } from '../../../core/Members.ts';
import { Avatar } from '../../common/Avatar.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

const ITEM_BASE = 'block w-full cursor-pointer rounded-[9px] px-2.5 py-2 text-left text-[12.5px] hover:bg-row';
const ITEM = `${ITEM_BASE} text-ink-icon hover:text-ink-strong`;
const DANGER_ITEM = `${ITEM_BASE} text-danger`;

export function MemberMenu() {
    const hub = useApp().hub;
    const { memberMenu, tree } = useStore(hub.store);
    const member = tree?.members.find(item => item.user_id === memberMenu?.userId);
    const [nickname, setNickname] = useState(member?.nickname ?? '');
    const [note, setNote] = useState('');
    const [banning, setBanning] = useState(false);
    const [reason, setReason] = useState('');
    const box = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const closeOutside = (event: MouseEvent) => {
            if (box.current && ! box.current.contains(event.target as Node)) {
                hub.closeMemberMenu();
            }
        };

        document.addEventListener('mousedown', closeOutside);

        return () => document.removeEventListener('mousedown', closeOutside);
    }, [hub]);

    useLayoutEffect(() => {
        const element = box.current;

        if (! element || ! memberMenu) {
            return;
        }

        element.style.left = `${Math.max(8, Math.min(memberMenu.x, window.innerWidth - element.offsetWidth - 8))}px`;
        element.style.top = `${Math.max(8, Math.min(memberMenu.y, window.innerHeight - element.offsetHeight - 8))}px`;
    }, [memberMenu, banning]);

    if (! member || ! memberMenu) {
        return null;
    }

    const actions = hub.memberActions(member);
    const roles = actions.roles ? hub.assignableRoles() : [];
    const nothing = ! Object.values(actions).some(Boolean);
    const self = member.user_id === hub.user?.id;
    const person = { id: member.user_id, name: member.name, avatar_url: member.avatar_url };
    const badges = (tree?.roles ?? []).filter(role => ! role.is_everyone && member.role_ids.includes(role.id));

    const message = async () => {
        if (note.trim() === '') {
            return;
        }

        if (await hub.direct.sendTo(person, note)) {
            setNote('');
            await hub.openDirect(person);
        }
    };

    return (
        <div ref={box} className="popover fixed z-[80] w-64 animate-rise p-2" style={{ left: memberMenu.x, top: memberMenu.y }}>
            <div className="flex items-center gap-2.5 px-2 pt-1 pb-2">
                <Avatar name={Members.displayName(member)} url={member.avatar_url} size={38} mine={self} />
                <span className="min-w-0">
                    <span className="block truncate text-[13.5px] font-semibold">{Members.displayName(member)}</span>
                    {member.nickname && <span className="block truncate text-[11px] text-ink-dim">{member.name}</span>}
                    {member.is_owner && <span className="label-mono mt-0.5 block text-[9.5px]">dono do servidor</span>}
                </span>
            </div>

            {badges.length > 0 && (
                <div className="flex flex-wrap gap-1 px-2 pb-2">
                    {badges.map(role => (
                        <span
                            key={role.id}
                            className="rounded-full border border-line-strong px-2 py-0.5 text-[10.5px]"
                            style={role.color ? { color: role.color, borderColor: role.color } : undefined}
                        >
                            {role.name}
                        </span>
                    ))}
                </div>
            )}

            {! self && (
                <form className="flex gap-1.5 border-t border-line px-1 py-2" onSubmit={event => { event.preventDefault(); void message(); }}>
                    <input
                        className="field min-w-0 flex-1 px-2.5 py-1.5 text-[12.5px]"
                        type="text"
                        maxLength={2000}
                        value={note}
                        onChange={event => setNote(event.target.value)}
                        placeholder={`Mensagem para ${member.name}`}
                    />
                    <button className="btn-primary px-2.5 py-1.5 text-[12px]" type="submit" disabled={note.trim() === ''}>Enviar</button>
                </form>
            )}

            {nothing && ! self && <p className="px-2.5 pb-2 text-[12px] text-ink-dim">Nada que você possa mudar nesta pessoa.</p>}

            {actions.nickname && (
                <form className="flex gap-1.5 px-1 pb-2" onSubmit={event => { event.preventDefault(); void hub.updateMember(member, { nickname: nickname.trim() || null }); }}>
                    <input className="field min-w-0 flex-1 px-2.5 py-1.5 text-[12.5px]" type="text" maxLength={32} value={nickname} onChange={event => setNickname(event.target.value)} placeholder="Apelido" />
                    <button className="btn-primary px-2.5 py-1.5 text-[12px]" type="submit">OK</button>
                </form>
            )}

            {roles.length > 0 && (
                <div className="scroll-thin max-h-40 overflow-y-auto border-t border-line px-2.5 py-2">
                    <p className="label-mono mb-1.5">Cargos</p>
                    {roles.map(role => (
                        <label key={role.id} className="flex cursor-pointer items-center gap-2 py-0.5 text-[12.5px]">
                            <input
                                className="accent-brand"
                                type="checkbox"
                                checked={member.role_ids.includes(role.id)}
                                onChange={event => void hub.updateMember(member, {
                                    role_ids: event.target.checked ? [...new Set([...member.role_ids, role.id])] : member.role_ids.filter(id => id !== role.id),
                                })}
                            />
                            <span style={{ color: role.color ?? undefined }}>{role.name}</span>
                        </label>
                    ))}
                </div>
            )}

            {(actions.mute || actions.deafen || actions.disconnect || actions.kick || actions.ban) && (
                <div className="mt-1 border-t border-line pt-1">
                    {actions.mute && <button className={ITEM} type="button" onClick={() => void hub.updateMember(member, { server_mute: ! member.server_mute })}>{member.server_mute ? 'Desmutar no servidor' : 'Mutar no servidor'}</button>}
                    {actions.deafen && <button className={ITEM} type="button" onClick={() => void hub.updateMember(member, { server_deaf: ! member.server_deaf })}>{member.server_deaf ? 'Devolver o áudio' : 'Ensurdecer no servidor'}</button>}
                    {actions.disconnect && <button className={ITEM} type="button" onClick={() => void hub.disconnectMember(member)}>Desconectar da sala de voz</button>}
                    {actions.kick && <button className={DANGER_ITEM} type="button" onClick={() => void hub.kickMember(member)}>Expulsar do servidor</button>}
                    {actions.ban && ! banning && <button className={DANGER_ITEM} type="button" onClick={() => setBanning(true)}>Banir do servidor…</button>}
                    {actions.ban && banning && (
                        <form className="flex gap-1.5 px-1 py-1.5" onSubmit={event => { event.preventDefault(); void hub.banMember(member, reason); }}>
                            <input className="field min-w-0 flex-1 px-2.5 py-1.5 text-[12.5px]" type="text" maxLength={200} value={reason} onChange={event => setReason(event.target.value)} placeholder="Motivo (opcional)" autoFocus />
                            <button className="btn-danger px-2.5 py-1.5 text-[12px] font-semibold" type="submit">Banir</button>
                        </form>
                    )}
                </div>
            )}
        </div>
    );
}
