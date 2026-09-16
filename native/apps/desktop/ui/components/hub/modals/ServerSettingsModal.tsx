import { useState } from 'react';

import { Permissions } from '../../../core/Permissions.ts';
import { Avatar } from '../../common/Avatar.tsx';
import { Icon } from '../../common/Icon.tsx';
import { Modal } from '../../common/Modal.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

const SMALL_BUTTON = 'btn-ghost px-2 py-1 text-[11.5px]';

export function ServerSettingsModal() {
    const hub = useApp().hub;
    const settings = hub.settings;
    const { tree, user, online } = useStore(hub.store);
    const [name, setName] = useState(tree?.name ?? '');

    if (! tree || ! user) {
        return null;
    }

    const owner = tree.owner_id === user.id;
    const roles = [...tree.roles].filter(role => ! role.is_everyone).sort((left, right) => right.position - left.position);
    const members = [...tree.members].sort((left, right) => (left.nickname ?? left.name).localeCompare(right.nickname ?? right.name));

    return (
        <Modal
            title="Configurações do servidor"
            subtitle={tree.name}
            width={520}
            onClose={() => hub.closeModal()}
            footer={(
                <>
                    {owner
                        ? <button className="btn-quiet" type="button" onClick={() => void settings.deleteServer()}>Excluir servidor</button>
                        : <button className="btn-quiet" type="button" onClick={() => void settings.leaveServer()}>Sair do servidor</button>}
                    <span className="flex-1" />
                    {hub.can(Permissions.MANAGE_SERVER)
                        ? <button className="btn-primary rounded-[10px] px-4 py-2.5 text-[12.5px] font-semibold" type="button" disabled={name.trim() === ''} onClick={() => void (name.trim() === tree.name ? hub.closeModal() : settings.rename(name))}>Salvar</button>
                        : <button className="btn-ghost" type="button" onClick={() => hub.closeModal()}>Fechar</button>}
                </>
            )}
        >
            <p className="label-mono mb-2">Ícone e nome</p>
            <form className="flex items-center gap-3" onSubmit={event => { event.preventDefault(); void settings.rename(name); }}>
                <Avatar name={tree.name} size={56} mine square />
                <input className="field min-w-0 flex-1" type="text" maxLength={60} value={name} disabled={! hub.can(Permissions.MANAGE_SERVER)} onChange={event => setName(event.target.value)} required />
            </form>

            {tree.invite_code && (
                <>
                    <p className="label-mono mt-6 mb-2">Convite</p>
                    <div className="flex items-center gap-2">
                        <code className="code-chip flex-1 py-2 text-[13px]">{tree.invite_code}</code>
                        <button className="btn-ghost flex items-center gap-1.5" type="button" onClick={() => void hub.copyInvite()}><Icon name="copy" size={13} />Copiar</button>
                        <button className="btn-ghost" type="button" title="Gera um código novo e invalida o antigo" onClick={() => void settings.regenerateInvite()}>Regenerar</button>
                    </div>
                </>
            )}

            <div className="mt-6 mb-2 flex items-center justify-between">
                <span className="label-mono">Membros</span>
                <span className="font-mono text-[10px] text-ink-dim">{members.length}</span>
            </div>
            <div className="flex flex-col gap-1.5">
                {members.map(member => {
                    const topRole = roles.find(role => member.role_ids.includes(role.id));
                    const me = member.user_id === user.id;

                    return (
                        <button key={member.user_id} className="row-item w-full cursor-pointer text-left" type="button" onClick={event => hub.openMemberMenu(member, event.clientX, event.clientY)} onContextMenu={event => { event.preventDefault(); hub.openMemberMenu(member, event.clientX, event.clientY); }}>
                            <Avatar name={member.nickname ?? member.name} size={26} mine={me} status={online.has(member.user_id) ? 'online' : 'offline'} />
                            <span className={`min-w-0 flex-1 truncate text-[13px] ${me ? 'text-ink-strong' : 'text-ink-icon'}`}>
                                {member.nickname ?? member.name}
                                {me && <span className="ml-1.5 font-mono text-[9.5px] text-ink-dim">você</span>}
                            </span>
                            {member.server_mute && <span className="text-danger" title="Mutado no servidor"><Icon name="micOff" size={13} /></span>}
                            {member.server_deaf && <span className="text-danger" title="Ensurdecido no servidor"><Icon name="headphonesOff" size={13} /></span>}
                            {topRole && <span className="size-[7px] rounded-full" style={{ background: topRole.color ?? 'var(--color-ink-dim)' }} />}
                            <span className={`font-mono text-[9.5px] ${member.is_owner ? 'text-lilac-2' : 'text-ink-dim'}`}>{member.is_owner ? 'dono' : topRole?.name ?? 'membro'}</span>
                            <Icon name="dots" size={13} className="text-ink-dim" />
                        </button>
                    );
                })}
            </div>

            {hub.can(Permissions.MANAGE_ROLES) && (
                <>
                    <div className="mt-6 mb-2 flex items-center justify-between">
                        <span className="label-mono">Cargos</span>
                        <button className={SMALL_BUTTON} type="button" onClick={() => hub.store.set({ roleEditor: { role: null } })}>Novo cargo</button>
                    </div>
                    <div className="flex flex-col gap-1.5">
                        {settings.roleRows().map(({ role, editable, up, down }) => (
                            <div key={role.id} className="row-item text-[13px]">
                                <span className="size-[8px] flex-none rounded-full" style={{ background: role.color ?? 'var(--color-ink-dim)' }} />
                                <span className="min-w-0 flex-1 truncate text-ink-body">{role.name}<span className="ml-1.5 font-mono text-[10px] text-ink-dim">· {role.position}</span></span>
                                {up !== null && <button className={SMALL_BUTTON} type="button" title="Subir" onClick={() => void settings.moveRole(role, up)}><Icon name="chevronDown" size={12} className="rotate-180" /></button>}
                                {down !== null && <button className={SMALL_BUTTON} type="button" title="Descer" onClick={() => void settings.moveRole(role, down)}><Icon name="chevronDown" size={12} /></button>}
                                {editable && <button className={SMALL_BUTTON} type="button" onClick={() => hub.store.set({ roleEditor: { role } })}>Editar</button>}
                                {editable && ! role.is_everyone && <button className={`${SMALL_BUTTON} hover:text-danger`} type="button" onClick={() => void settings.deleteRole(role)}>Apagar</button>}
                            </div>
                        ))}
                    </div>
                </>
            )}

            {hub.can(Permissions.BAN_MEMBERS) && (
                <>
                    <p className="label-mono mt-6 mb-2">Banidos</p>
                    {! tree.bans?.length && <p className="text-[12.5px] text-ink-dim">Ninguém banido.</p>}
                    <div className="flex flex-col gap-1.5">
                        {(tree.bans ?? []).map(ban => (
                            <div key={ban.user_id} className="row-item text-[13px]">
                                <span className="min-w-0 flex-1 truncate text-ink-body">{ban.name}</span>
                                <span className="min-w-0 flex-1 truncate text-[12px] text-ink-dim">{ban.reason ?? ''}</span>
                                <button className={SMALL_BUTTON} type="button" onClick={() => void settings.unban(ban)}>Perdoar</button>
                            </div>
                        ))}
                    </div>
                </>
            )}
        </Modal>
    );
}
