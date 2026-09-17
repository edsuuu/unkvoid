import { useState } from 'react';

import { Avatar } from '../common/Avatar.tsx';
import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { AuthCard } from './AuthCard.tsx';

type EntryAction = 'create' | 'join';

export function EntryScreen() {
    const app = useApp();
    const { entryName, entryCode, entryError } = useStore(app.store);
    const { user } = useStore(app.hub.store);
    const [busy, setBusy] = useState<EntryAction | null>(null);

    const run = async (kind: EntryAction, work: () => Promise<unknown>) => {
        setBusy(kind);

        try {
            await work();
        } finally {
            setBusy(null);
        }
    };

    const create = () => {
        if (busy === null) {
            void run('create', () => app.createRoom());
        }
    };

    return (
        <div className="scroll-thin flex h-full items-start justify-center overflow-y-auto p-6 pt-[10vh]">
            <div className="flex w-full max-w-[880px] flex-wrap items-stretch justify-center gap-5">
                <div className="glass flex w-full max-w-[420px] animate-rise flex-col rounded-[22px] p-8 shadow-[0_30px_80px_-40px_rgba(0,0,0,0.9)]">
                    <p className="text-center text-lg font-semibold tracking-tight">Criar uma sala</p>
                    <p className="mt-1.5 mb-6 text-center text-[13px] text-ink-soft">Compartilhe sua tela com quem você quiser.</p>

                    {user
                        ? (
                            <div className="mb-5 flex flex-col items-center gap-2.5">
                                <Avatar name={user.name} size={54} mine />
                                <span className="text-[15px] font-semibold">{user.name}</span>
                            </div>
                        )
                        : (
                            <label className="mb-3.5 block">
                                <span className="label-mono mb-2 block">Seu nome</span>
                                <input className="field w-full" type="text" maxLength={40} value={entryName} onChange={event => app.setEntry({ entryName: event.target.value })} placeholder="Como aparecer para os outros" autoComplete="off" autoFocus onKeyDown={event => event.key === 'Enter' && create()} />
                            </label>
                        )}

                    <button className="btn-primary flex w-full items-center justify-center gap-2 text-[14.5px]" type="button" disabled={busy !== null} onClick={create}>
                        {busy === 'create' && <Spinner size={15} />}
                        {user ? 'Criar uma sala' : 'Criar uma sala sem login'}
                    </button>

                    <div className="divider-or label-mono my-4">ou</div>

                    <form className="flex gap-2" onSubmit={event => { event.preventDefault(); void run('join', () => app.joinRoom()); }}>
                        <input className="field min-w-0 flex-1 font-mono placeholder:font-sans" type="text" maxLength={32} value={entryCode} onChange={event => app.setEntry({ entryCode: event.target.value })} placeholder="Código da sala" autoComplete="off" spellCheck="false" />
                        <button className="btn-ghost flex items-center gap-2 px-4 text-[13.5px] font-medium" type="submit" disabled={busy !== null}>{busy === 'join' && <Spinner size={13} />}Entrar</button>
                    </form>

                    <p className="mt-3 min-h-5 text-center text-[12.5px] text-danger" role="alert">{entryError}</p>

                    {user && (
                        <button className="btn-ghost mt-auto w-full py-2.5" type="button" onClick={() => void app.hub.open()}>Voltar aos servidores</button>
                    )}
                </div>

                {! user && <AuthCard />}
            </div>
        </div>
    );
}
