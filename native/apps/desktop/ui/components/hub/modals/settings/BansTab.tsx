import { useApp } from '../../../useApp.ts';
import { useStore } from '../../../useStore.ts';

export function BansTab() {
    const hub = useApp().hub;
    const { tree } = useStore(hub.store);
    const bans = tree?.bans ?? [];

    return (
        <>
            <p className="label-mono mb-2">Banidos</p>

            {bans.length === 0 && <p className="text-[12.5px] text-ink-dim">Ninguém banido.</p>}

            <div className="flex flex-col gap-1.5">
                {bans.map(ban => (
                    <div key={ban.user_id} className="row-item text-[13px]">
                        <span className="min-w-0 flex-1 truncate text-ink-body">{ban.name}</span>
                        <span className="min-w-0 flex-1 truncate text-[12px] text-ink-dim">{ban.reason ?? ''}</span>
                        <button className="btn-ghost px-2 py-1 text-[11.5px]" type="button" onClick={() => void hub.settings.unban(ban)}>Perdoar</button>
                    </div>
                ))}
            </div>
        </>
    );
}
