import { useEffect } from 'react';

import { Avatar } from '../../../common/Avatar.tsx';
import { useApp } from '../../../useApp.ts';
import { useStore } from '../../../useStore.ts';

export function AuditTab() {
    const settings = useApp().hub.settings;
    const { entries, loading, failed } = useStore(settings.audits);

    useEffect(() => {
        void settings.loadAudits();
    }, [settings]);

    return (
        <>
            <p className="label-mono mb-2">Auditoria</p>

            {loading && [0, 1, 2].map(index => <div key={index} className="skeleton mb-1.5 h-10" />)}

            {! loading && failed && (
                <div className="flex flex-col items-center gap-2 py-6 text-center">
                    <p className="text-[13px] text-danger">Não deu para carregar a auditoria.</p>
                    <button className="btn-ghost" type="button" onClick={() => void settings.loadAudits()}>Tentar de novo</button>
                </div>
            )}

            {! loading && ! failed && entries.length === 0 && <p className="text-[12.5px] text-ink-dim">Nada registrado ainda.</p>}

            <div className="flex flex-col gap-1.5">
                {entries.map(entry => (
                    <div key={entry.id} className="row-item items-start text-[13px]">
                        <Avatar name={entry.actor?.name ?? '?'} url={entry.actor?.avatar_url ?? null} size={26} />
                        <span className="min-w-0 flex-1">
                            <span className="block truncate text-[12.5px] text-ink-body">
                                <span className="font-medium">{entry.actor?.name ?? 'alguém'}</span> {entry.summary}
                            </span>
                            <span className="block font-mono text-[9.5px] text-ink-dim">{new Date(entry.at).toLocaleString('pt-BR')}</span>
                        </span>
                    </div>
                ))}
            </div>
        </>
    );
}
