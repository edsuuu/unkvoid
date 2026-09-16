import { memo, useState } from 'react';

import type { Message } from '../../core/Models.ts';
import { Avatar } from '../common/Avatar.tsx';
import { useApp } from '../useApp.ts';

const TIME_FORMAT: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit', day: '2-digit', month: '2-digit' };

type MessageRowProps = {
    message: Message;
    mine: boolean;
    canDelete: boolean;
};

export const MessageRow = memo(function MessageRow({ message, mine, canDelete }: MessageRowProps) {
    const chat = useApp().hub.chat;
    const [editing, setEditing] = useState(false);
    const [draft, setDraft] = useState(message.body);
    const when = new Date(message.created_at).toLocaleString('pt-BR', TIME_FORMAT);

    return (
        <div className={`group flex animate-fade-in gap-2.5 ${mine ? 'flex-row-reverse' : ''}`}>
            <Avatar name={message.user.name} size={28} mine={mine} />
            <div className={`flex min-w-0 max-w-[75%] flex-col ${mine ? 'items-end' : 'items-start'}`}>
                <p className="text-[11px] text-ink-dim">
                    {message.user.name} · {when}{message.edited_at ? ' (editada)' : ''}
                </p>

                {editing
                    ? (
                        <textarea
                            className="field mt-1 w-full min-w-64 resize-none px-2.5 py-1.5 text-[13px]"
                            rows={Math.min(8, draft.split('\n').length)}
                            maxLength={2000}
                            value={draft}
                            autoFocus
                            onFocus={event => event.target.setSelectionRange(event.target.value.length, event.target.value.length)}
                            onChange={event => setDraft(event.target.value)}
                            onKeyDown={event => {
                                if (event.key === 'Escape') {
                                    event.stopPropagation();
                                    setDraft(message.body);
                                    setEditing(false);
                                }

                                if (event.key === 'Enter' && ! event.shiftKey) {
                                    event.preventDefault();
                                    setEditing(false);
                                    void chat.edit(message, draft).then(saved => saved || setEditing(true));
                                }
                            }}
                        />
                    )
                    : <p className={`bubble mt-1 whitespace-pre-wrap break-words ${mine ? 'bubble-mine' : ''}`}>{message.body}</p>}
            </div>

            {! editing && (mine || canDelete) && (
                <span className="flex flex-none gap-1 self-center opacity-0 group-hover:opacity-100 focus-within:opacity-100">
                    {mine && <button className="cursor-pointer rounded-md px-2 py-1 text-[11px] text-ink-dim hover:bg-row hover:text-ink-strong" type="button" onClick={() => { setDraft(message.body); setEditing(true); }}>Editar</button>}
                    {canDelete && <button className="cursor-pointer rounded-md px-2 py-1 text-[11px] text-ink-dim hover:bg-row hover:text-danger" type="button" onClick={() => void chat.destroy(message)}>Apagar</button>}
                </span>
            )}
        </div>
    );
});
