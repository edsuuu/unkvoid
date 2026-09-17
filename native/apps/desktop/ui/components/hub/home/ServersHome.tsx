import { useState, type FormEvent } from 'react';

import { Avatar } from '../../common/Avatar.tsx';
import { Spinner } from '../../common/Spinner.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

export function ServersHome() {
    const app = useApp();
    const hub = app.hub;
    const recentRooms = app.recentRooms();
    const { servers, serversLoading, serversFailed, user } = useStore(hub.store);
    const [name, setName] = useState('');
    const [busy, setBusy] = useState(false);

    const create = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        setBusy(true);

        if (await hub.attempt(() => hub.createServer(name))) {
            setName('');
        }

        setBusy(false);
    };

    return (
        <div className="scroll-thin flex min-w-0 flex-1 flex-wrap content-start gap-3 overflow-y-auto">
            <form className="glass flex min-w-[260px] flex-[0_1_360px] animate-rise flex-col gap-3 p-5" onSubmit={create}>
                <p className="label-mono">Home</p>
                <p className="text-[17px] font-semibold tracking-tight">Oi, {user?.name}.</p>
                <p className="text-[13px] text-ink-soft">Uma sala nova já vem com um canal de texto e um de voz. Depois é só mandar o convite.</p>
                <input className="field w-full" type="text" maxLength={60} value={name} onChange={event => setName(event.target.value)} placeholder="Nome da sala" required />
                <button className="btn-primary flex items-center justify-center gap-2 text-[13.5px]" type="submit" disabled={busy}>
                    {busy && <Spinner size={14} />}
                    Criar sala
                </button>
                <button className="btn-ghost" type="button" onClick={() => hub.openModal({ type: 'server' })}>Tenho um convite</button>
            </form>

            <div className="glass flex min-w-[260px] flex-[0_1_360px] animate-rise flex-col gap-3 p-5">
                <p className="label-mono">Só compartilhar a tela</p>
                <p className="text-[13px] text-ink-soft">Uma sala por código, sem servidor: quem tiver o código assiste.</p>
                <button className="btn-primary text-[13.5px]" type="button" onClick={() => void hub.roomByCode()}>Criar ou entrar com código</button>

                {recentRooms.length > 0 && (
                    <>
                        <p className="label-mono mt-1">Últimas salas acessadas</p>
                        <div className="flex flex-wrap gap-2">
                            {recentRooms.map(code => (
                                <button key={code} className="btn-ghost px-3 py-1.5 font-mono text-[12.5px]" type="button" onClick={() => void app.openRoom(code)}>{code}</button>
                            ))}
                        </div>
                    </>
                )}
            </div>

            <div className="glass flex min-w-[280px] flex-1 animate-rise flex-col gap-2 p-5">
                <p className="label-mono mb-1">Últimas salas</p>

                {serversLoading && servers.length === 0 && [0, 1, 2].map(index => <div key={index} className="skeleton h-12" />)}

                {! serversLoading && servers.length === 0 && serversFailed && (
                    <div className="flex flex-col items-center gap-2 py-6 text-center">
                        <p className="text-[13px] text-danger">Não deu para carregar as suas salas.</p>
                        <button className="btn-ghost" type="button" onClick={() => void hub.attempt(() => hub.loadServers())}>Tentar de novo</button>
                    </div>
                )}

                {! serversLoading && servers.length === 0 && ! serversFailed && (
                    <p className="py-6 text-center text-[13px] text-ink-dim">Nenhuma ainda. Crie uma ao lado ou entre com um convite.</p>
                )}

                {servers.map(server => (
                    <button key={server.id} className="row-item w-full cursor-pointer text-left" type="button" onClick={() => void hub.attempt(() => hub.openServer(server.id))}>
                        <Avatar name={server.name} url={server.icon_url} size={32} square />
                        <span className="min-w-0 flex-1">
                            <span className="block truncate text-[13.5px] font-semibold">{server.name}</span>
                            <span className="label-mono mt-0.5 block text-[9.5px]">{server.owner_id === user?.id ? 'dono' : 'membro'}</span>
                        </span>
                        <span className="font-mono text-[10.5px] text-ink-dim">
                            {server.last_accessed_at ? new Date(server.last_accessed_at).toLocaleDateString('pt-BR') : 'nunca na voz'}
                        </span>
                    </button>
                ))}
            </div>
        </div>
    );
}
