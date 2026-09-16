import { memo, useState } from 'react';

import type { Message } from '../../core/Models.ts';
import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
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
    const [menuOpen, setMenuOpen] = useState(false);
    const [draft, setDraft] = useState(message.body);
    const when = new Date(message.created_at).toLocaleString('pt-BR', TIME_FORMAT);

    return (
        <div className={`group flex animate-fade-in gap-2.5 ${mine ? 'flex-row-reverse' : ''}`}>
            <Avatar name={message.user.name} size={28} mine={mine} />
            <div className={`flex min-w-0 max-w-[75%] flex-col ${mine ? 'items-end' : 'items-start'}`}>
                <p className="text-[11px] text-ink-dim">
                    {message.user.name} · {when}
                    {message.edited_at && <span className="ml-1.5 rounded-full bg-row px-1.5 py-0.5 text-[10px] text-ink-icon">editado</span>}
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
                <span className="relative flex-none self-center">
                    <button
                        className={`btn-icon size-7 rounded-lg transition ${menuOpen ? 'btn-icon-on' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100'}`}
                        type="button"
                        title="Opções da mensagem"
                        onClick={() => setMenuOpen(open => ! open)}
                    >
                        <Icon name="dots" size={15} />
                    </button>

                    <Popover open={menuOpen} onClose={() => setMenuOpen(false)} className={`top-9 w-40 ${mine ? 'left-0' : 'right-0'}`}>
                        {mine && (
                            <button
                                className="flex w-full cursor-pointer items-center gap-2 rounded-[9px] px-2.5 py-2 text-left text-[12.5px] text-ink-icon hover:bg-row hover:text-ink-strong"
                                type="button"
                                onClick={() => { setMenuOpen(false); setDraft(message.body); setEditing(true); }}
                            >
                                <Icon name="edit" size={14} />
                                Editar
                            </button>
                        )}
                        {canDelete && (
                            <button
                                className="flex w-full cursor-pointer items-center gap-2 rounded-[9px] px-2.5 py-2 text-left text-[12.5px] text-ink-icon hover:bg-row hover:text-danger"
                                type="button"
                                onClick={() => { setMenuOpen(false); void chat.destroy(message); }}
                            >
                                <Icon name="trash" size={14} />
                                Apagar
                            </button>
                        )}
                    </Popover>
                </span>
            )}
        </div>
    );
});
