import { useState } from 'react';

import { Modal } from '../../common/Modal.tsx';
import { Spinner } from '../../common/Spinner.tsx';
import { useApp } from '../../useApp.ts';

type ServerAction = 'create' | 'join';

export function ServerModal() {
    const hub = useApp().hub;
    const [name, setName] = useState('');
    const [code, setCode] = useState('');
    const [busy, setBusy] = useState<ServerAction | null>(null);

    const run = async (kind: ServerAction, work: () => Promise<unknown>) => {
        setBusy(kind);

        const done = await hub.attempt(work);

        setBusy(null);

        if (done) {
            hub.closeModal();
        }
    };

    return (
        <Modal title="Criar um servidor" subtitle="Vem com um canal de texto e um de voz." width={460} onClose={() => hub.closeModal()}>
            <form className="flex gap-2" onSubmit={event => { event.preventDefault(); void run('create', () => hub.createServer(name)); }}>
                <input className="field min-w-0 flex-1" type="text" maxLength={60} value={name} onChange={event => setName(event.target.value)} placeholder="Nome do servidor" autoFocus />
                <button className="btn-primary flex items-center gap-2 px-4 text-[13.5px]" type="submit" disabled={Boolean(busy)}>
                    {busy === 'create' && <Spinner size={14} />}
                    Criar
                </button>
            </form>

            <div className="divider-or label-mono my-5">ou</div>

            <p className="mb-3 text-[15px] font-semibold">Entrar com um convite</p>
            <form className="flex gap-2" onSubmit={event => { event.preventDefault(); void run('join', () => hub.joinInvite(code)); }}>
                <input className="field min-w-0 flex-1 font-mono placeholder:font-sans" type="text" maxLength={32} value={code} onChange={event => setCode(event.target.value)} placeholder="Código do convite" autoComplete="off" spellCheck="false" />
                <button className="btn-ghost flex items-center gap-2 px-4 text-[13.5px]" type="submit" disabled={Boolean(busy)}>
                    {busy === 'join' && <Spinner size={14} />}
                    Entrar
                </button>
            </form>
        </Modal>
    );
}
