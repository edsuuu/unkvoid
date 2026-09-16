import { useRef, useState } from 'react';

import { Permissions } from '../../../../core/Permissions.ts';
import { Avatar } from '../../../common/Avatar.tsx';
import { Icon } from '../../../common/Icon.tsx';
import { useApp } from '../../../useApp.ts';
import { useStore } from '../../../useStore.ts';

export function OverviewTab({ name, onName }: { name: string; onName: (value: string) => void }) {
    const hub = useApp().hub;
    const settings = hub.settings;
    const { tree } = useStore(hub.store);
    const picker = useRef<HTMLInputElement>(null);
    const [busy, setBusy] = useState(false);

    if (! tree) {
        return null;
    }

    const manage = hub.can(Permissions.MANAGE_SERVER);

    const choose = async (file: File | undefined) => {
        if (! file) {
            return;
        }

        setBusy(true);
        await settings.uploadIcon(file);
        setBusy(false);
    };

    return (
        <>
            <p className="label-mono mb-2">Ícone e nome</p>
            <form className="flex items-center gap-3" onSubmit={event => { event.preventDefault(); void settings.rename(name); }}>
                <button
                    className="relative cursor-pointer rounded-[18px] border-none bg-transparent p-0 disabled:cursor-default"
                    type="button"
                    title={manage ? 'Trocar o ícone do servidor' : 'Só quem gerencia o servidor troca o ícone'}
                    disabled={! manage || busy}
                    onClick={() => picker.current?.click()}
                >
                    <Avatar name={tree.name} url={tree.icon_url} size={56} mine square />
                    {manage && (
                        <span className="absolute inset-0 flex items-center justify-center rounded-[18px] bg-[rgba(6,5,10,0.55)] text-white opacity-0 transition hover:opacity-100">
                            <Icon name="edit" size={16} />
                        </span>
                    )}
                </button>

                <input
                    ref={picker}
                    className="hidden"
                    type="file"
                    accept="image/png,image/jpeg,image/webp"
                    onChange={event => { void choose(event.target.files?.[0]); event.target.value = ''; }}
                />

                <span className="flex min-w-0 flex-1 flex-col gap-1.5">
                    <input className="field w-full" type="text" maxLength={60} value={name} disabled={! manage} onChange={event => onName(event.target.value)} required />
                    {manage && tree.icon_url && (
                        <button className="cursor-pointer self-start text-[11.5px] text-ink-dim hover:text-danger" type="button" onClick={() => void settings.removeIcon()}>Remover o ícone</button>
                    )}
                </span>
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
        </>
    );
}
