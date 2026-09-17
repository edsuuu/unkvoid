import { useState, type FormEvent } from 'react';

import type { Friendship } from '../../../core/Models.ts';
import { Avatar } from '../../common/Avatar.tsx';
import { Icon } from '../../common/Icon.tsx';
import { Spinner } from '../../common/Spinner.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

type FriendsTab = 'accepted' | 'pending' | 'blocked';

const TABS: [FriendsTab, string][] = [['accepted', 'Amigos'], ['pending', 'Pendentes'], ['blocked', 'Bloqueados']];

export function FriendsPanel() {
    const hub = useApp().hub;
    const friends = hub.friends;
    const { loading, failed } = useStore(friends.store);
    const [tab, setTab] = useState<FriendsTab>('accepted');
    const [email, setEmail] = useState('');
    const [busy, setBusy] = useState(false);

    const incoming = friends.incoming();
    const outgoing = friends.outgoing();
    const rows = tab === 'accepted' ? friends.accepted() : tab === 'blocked' ? friends.blocked() : [...incoming, ...outgoing];

    const add = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        setBusy(true);

        if (await friends.request(email.trim())) {
            setEmail('');
        }

        setBusy(false);
    };

    const row = (friendship: Friendship) => {
        const person = friends.other(friendship);
        const waiting = outgoing.includes(friendship);

        return (
            <div key={friendship.id} className="row-item">
                <Avatar name={person.name} url={person.avatar_url} size={30} />
                <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-medium">{person.name}</span>
                    {tab === 'pending' && <span className="block text-[11.5px] text-ink-dim">{waiting ? 'aguardando resposta' : 'quer ser seu amigo'}</span>}
                </span>

                {tab === 'accepted' && (
                    <button className="btn-icon size-7 rounded-[8px]" type="button" title="Mandar mensagem" onClick={() => void hub.direct.open(person)}>
                        <Icon name="chat" size={13} />
                    </button>
                )}

                {tab === 'pending' && ! waiting && (
                    <button className="btn-ghost px-2.5 py-1.5 text-[12px]" type="button" onClick={() => void friends.accept(friendship)}>Aceitar</button>
                )}

                {tab !== 'blocked' && (
                    <button className="btn-icon size-7 rounded-[8px]" type="button" title="Bloquear" onClick={() => void friends.block(friendship)}>
                        <Icon name="close" size={13} />
                    </button>
                )}

                <button className="btn-icon size-7 rounded-[8px] hover:text-danger" type="button" title="Desfazer" onClick={() => void friends.remove(friendship)}>
                    <Icon name="trash" size={13} />
                </button>
            </div>
        );
    };

    return (
        <section className="glass flex min-h-0 flex-1 animate-fade-in flex-col gap-3 p-4">
            <div className="flex items-center gap-1.5 border-b border-line pb-3">
                {TABS.map(([key, label]) => (
                    <button key={key} className={`tab-soft ${tab === key ? 'tab-soft-on' : ''}`} type="button" onClick={() => setTab(key)}>
                        {label}
                        {key === 'pending' && incoming.length > 0 && <span className="ml-1.5 rounded-full bg-danger px-1.5 py-0.5 text-[10px] font-semibold text-white">{incoming.length}</span>}
                    </button>
                ))}
            </div>

            <form className="flex items-center gap-2" onSubmit={add} noValidate>
                <input
                    className="field min-w-0 flex-1 py-2.5 text-[13px]"
                    type="email"
                    maxLength={255}
                    value={email}
                    placeholder="E-mail de quem você quer adicionar"
                    onChange={event => setEmail(event.target.value)}
                />
                <button className="btn-primary flex items-center gap-2 px-4 py-2.5 text-[13px]" type="submit" disabled={busy}>
                    {busy && <Spinner size={13} />}
                    Adicionar
                </button>
            </form>

            <div className="scroll-thin flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto pr-1">
                {loading && [0, 1].map(index => <div key={index} className="skeleton h-12" />)}

                {! loading && failed && (
                    <div className="m-auto flex flex-col items-center gap-2 text-center">
                        <p className="text-[13px] text-danger">Não deu para carregar os seus amigos.</p>
                        <button className="btn-ghost" type="button" onClick={() => void friends.load()}>Tentar de novo</button>
                    </div>
                )}

                {! loading && ! failed && rows.length === 0 && (
                    <p className="m-auto text-center text-[13px] text-ink-dim">
                        {tab === 'accepted' ? 'Você ainda não tem amigos por aqui. Adicione pelo e-mail.' : tab === 'pending' ? 'Nenhum pedido pendente.' : 'Ninguém bloqueado.'}
                    </p>
                )}

                {rows.map(row)}
            </div>
        </section>
    );
}
