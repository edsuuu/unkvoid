import { useState } from 'react';

import type { Role } from '../../../core/Models.ts';
import { Permissions } from '../../../core/Permissions.ts';
import { Modal } from '../../common/Modal.tsx';
import { useApp } from '../../useApp.ts';

export function RoleModal({ role }: { role: Role | null }) {
    const hub = useApp().hub;
    const settings = hub.settings;
    const everyone = Boolean(role?.is_everyone);
    const [name, setName] = useState(role?.name ?? '');
    const [color, setColor] = useState(role?.color ?? '#8a7cf5');
    const [permissions, setPermissions] = useState(role?.permissions ?? 0);
    const close = () => hub.store.set({ roleEditor: null });

    return (
        <Modal
            title={role ? `Cargo: ${role.name}` : 'Novo cargo'}
            width={520}
            onClose={close}
            footer={(
                <>
                    {role && ! everyone && <button className="btn-danger" type="button" onClick={() => void settings.deleteRole(role)}>Apagar</button>}
                    <span className="flex-1" />
                    <button className="btn-ghost" type="button" onClick={close}>Cancelar</button>
                    <button className="btn-primary px-4 py-2 text-[13px]" type="submit" form="role-form">Salvar</button>
                </>
            )}
        >
            <form id="role-form" onSubmit={event => { event.preventDefault(); void settings.saveRole(role, { name, color, permissions }); }}>
                <div className="flex gap-2">
                    <input className="field min-w-0 flex-1" type="text" maxLength={40} value={name} disabled={everyone} onChange={event => setName(event.target.value)} placeholder="Nome do cargo" required />
                    <input className="field size-11 cursor-pointer p-1" type="color" value={color} disabled={everyone} onChange={event => setColor(event.target.value)} title="Cor" />
                </div>

                <p className="label-mono mt-5 mb-2">Permissões</p>
                <div className="grid grid-cols-2 gap-x-4 gap-y-2 text-[13px] text-ink-icon">
                    {Permissions.LABELS.map(([flag, label]) => (
                        <label key={flag} className="flex cursor-pointer items-center gap-2">
                            <input
                                className="size-4 cursor-pointer accent-brand"
                                type="checkbox"
                                checked={(permissions & Permissions[flag]) !== 0}
                                onChange={event => setPermissions(bits => (event.target.checked ? bits | Permissions[flag] : bits & ~Permissions[flag]))}
                            />
                            {label}
                        </label>
                    ))}
                </div>
            </form>
        </Modal>
    );
}
