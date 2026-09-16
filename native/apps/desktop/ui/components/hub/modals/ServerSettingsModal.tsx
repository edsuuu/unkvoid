import { useState } from 'react';

import { Permissions } from '../../../core/Permissions.ts';
import { Icon, type IconName } from '../../common/Icon.tsx';
import { Modal } from '../../common/Modal.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';
import { AuditTab } from './settings/AuditTab.tsx';
import { BansTab } from './settings/BansTab.tsx';
import { MembersTab } from './settings/MembersTab.tsx';
import { OverviewTab } from './settings/OverviewTab.tsx';
import { RolesTab } from './settings/RolesTab.tsx';

type SettingsTab = 'overview' | 'members' | 'roles' | 'bans' | 'audit';

const TABS: [SettingsTab, string, IconName, number | null][] = [
    ['overview', 'Visão geral', 'gear', null],
    ['members', 'Membros', 'users', null],
    ['roles', 'Cargos', 'crown', Permissions.MANAGE_ROLES],
    ['bans', 'Banidos', 'logout', Permissions.BAN_MEMBERS],
    ['audit', 'Auditoria', 'logs', Permissions.VIEW_AUDIT_LOG],
];

export function ServerSettingsModal() {
    const hub = useApp().hub;
    const settings = hub.settings;
    const { tree, user } = useStore(hub.store);
    const [name, setName] = useState(tree?.name ?? '');
    const [tab, setTab] = useState<SettingsTab>('overview');

    if (! tree || ! user) {
        return null;
    }

    const owner = tree.owner_id === user.id;
    const allowed = TABS.filter(([, , , permission]) => permission === null || hub.can(permission));
    const shown = allowed.some(([key]) => key === tab) ? tab : 'overview';

    return (
        <Modal
            title="Configurações do servidor"
            subtitle={tree.name}
            width={760}
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
            <div className="flex min-h-[320px] gap-5">
                <nav className="flex w-[170px] flex-none flex-col gap-1 border-r border-line pr-3">
                    {allowed.map(([key, label, icon]) => (
                        <button
                            key={key}
                            className={`row-item w-full cursor-pointer text-left ${shown === key ? 'row-item-on' : ''}`}
                            type="button"
                            onClick={() => setTab(key)}
                        >
                            <Icon name={icon} size={14} />
                            <span className="flex-1 text-[12.5px]">{label}</span>
                        </button>
                    ))}
                </nav>

                <div className="min-w-0 flex-1">
                    {shown === 'overview' && <OverviewTab name={name} onName={setName} />}
                    {shown === 'members' && <MembersTab />}
                    {shown === 'roles' && <RolesTab />}
                    {shown === 'bans' && <BansTab />}
                    {shown === 'audit' && <AuditTab />}
                </div>
            </div>
        </Modal>
    );
}
