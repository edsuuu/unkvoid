import { useLayoutEffect, useRef, useState } from 'react';

import { Avatar } from '../../common/Avatar.tsx';
import { Icon } from '../../common/Icon.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

export function DirectPanel() {
    const hub = useApp().hub;
    const direct = hub.direct;
    const { person, messages, loading, failed } = useStore(direct.store);
    const [draft, setDraft] = useState('');
    const list = useRef<HTMLDivElement>(null);

    useLayoutEffect(() => {
        if (list.current) {
            list.current.scrollTop = list.current.scrollHeight;
        }
    }, [messages]);

    if (! person) {
        return null;
    }

    const send = async () => {
        const body = draft;

        if (body.trim() === '') {
            return;
        }

        setDraft('');

        if (! await direct.send(body)) {
            setDraft(current => (current === '' ? body : current));
        }
    };

    return (
        <section className="glass flex min-h-0 flex-1 animate-fade-in flex-col gap-3 p-4">
            <div className="flex items-center gap-2.5 border-b border-line pb-3">
                <Avatar name={person.name} url={person.avatar_url} size={26} />
                <span className="text-[14px] font-semibold">{person.name}</span>
                <span className="flex-1" />
                <button className="cursor-pointer p-1 text-ink-dim hover:text-ink-strong" type="button" title="Fechar a conversa" onClick={() => direct.close()}>
                    <Icon name="close" size={14} />
                </button>
            </div>

            <div ref={list} className="scroll-thin flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto pr-1">
                {loading && [0, 1, 2].map(index => <div key={index} className="skeleton h-12 w-2/5 rounded-xl" />)}

                {! loading && failed && (
                    <div className="m-auto flex flex-col items-center gap-2 text-center">
                        <p className="text-[13px] text-danger">Não deu para carregar esta conversa.</p>
                        <button className="btn-ghost" type="button" onClick={() => void direct.open(person)}>Tentar de novo</button>
                    </div>
                )}

                {! loading && ! failed && messages.length === 0 && (
                    <p className="m-auto text-center text-[13px] text-ink-dim">Nenhuma mensagem ainda com {person.name}.</p>
                )}

                {messages.map(message => (
                    <div key={message.id} className={`flex gap-2.5 ${message.mine ? 'flex-row-reverse' : ''}`}>
                        <Avatar name={message.sender.name} url={message.sender.avatar_url} size={28} mine={message.mine} />
                        <div className={`bubble ${message.mine ? 'bubble-mine' : ''}`}>
                            <p className="m-0 text-[13px] break-words whitespace-pre-wrap">{message.body}</p>
                            <span className="mt-1 block font-mono text-[9.5px] text-ink-dim">
                                {new Date(message.created_at).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })}
                                {message.edited_at && ' · editado'}
                            </span>
                        </div>
                    </div>
                ))}
            </div>

            <form className="flex items-end gap-2" onSubmit={event => { event.preventDefault(); void send(); }}>
                <input
                    className="field min-w-0 flex-1 py-2.5 text-[13px]"
                    type="text"
                    maxLength={2000}
                    value={draft}
                    placeholder={`Escreva para ${person.name}`}
                    onChange={event => setDraft(event.target.value)}
                />
                <button className="btn-primary px-4 py-2.5 text-[13px]" type="submit" disabled={draft.trim() === ''}>Enviar</button>
            </form>
        </section>
    );
}
