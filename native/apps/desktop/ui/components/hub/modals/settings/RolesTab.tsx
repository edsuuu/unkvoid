import { Icon } from '../../../common/Icon.tsx';
import { useApp } from '../../../useApp.ts';
import { useStore } from '../../../useStore.ts';

const SMALL_BUTTON = 'btn-ghost px-2 py-1 text-[11.5px]';

export function RolesTab() {
    const hub = useApp().hub;
    const settings = hub.settings;

    useStore(hub.store);

    return (
        <>
            <div className="mb-2 flex items-center justify-between">
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
    );
}
