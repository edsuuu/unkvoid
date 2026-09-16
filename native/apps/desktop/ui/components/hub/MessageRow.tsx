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

    if (message.type === 'join') {
        return (
            <p className="flex animate-fade-in items-center justify-center gap-2 py-0.5 text-[12px] text-ink-dim">
                <Icon name="users" size={13} />
                <span><span className="text-ink-icon">@{message.user.name}</span> chegou no servidor</span>
                <span className="font-mono text-[9.5px]">{when}</span>
            </p>
        );
    }

    const cancelEdit = () => {
        setDraft(message.body);
        setEditing(false);
    };

    const saveEdit = () => {
        if (draft.trim() === '' || draft === message.body) {
            cancelEdit();

            return;
        }

        setEditing(false);
        void chat.edit(message, draft).then(saved => saved || setEditing(true));
    };

    const actions = ! editing && (mine || canDelete) && (
        <span className="relative flex-none self-center">
            <button
                className={`btn-icon size-7 rounded-lg transition ${menuOpen ? 'btn-icon-on' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100'}`}
                type="button"
                title="Opções da mensagem"
                onClick={() => setMenuOpen(open => ! open)}
            >
                <Icon name="dots" size={15} />
            </button>

            <Popover open={menuOpen} onClose={() => setMenuOpen(false)} className={`top-9 w-40 ${mine ? 'right-0' : 'left-0'}`}>
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
    );

    return (
        <div className={`group flex animate-fade-in gap-2.5 ${mine ? 'flex-row-reverse' : ''}`}>
            <Avatar name={message.user.name} url={message.user.avatar_url} size={28} mine={mine} />
            {mine && actions}
            <div className={`flex min-w-0 max-w-[75%] flex-col ${mine ? 'items-end' : 'items-start'}`}>
                <p className="text-[11px] text-ink-dim">
                    {message.user.name} · {when}
                    {message.edited_at && <span className="ml-1.5 rounded-full bg-row px-1.5 py-0.5 text-[10px] text-ink-icon">editado</span>}
                </p>

                {editing
                    ? (
                        <span className="mt-1 flex w-full flex-col items-end gap-1.5">
                            <textarea
                                className="field w-full min-w-64 resize-none px-2.5 py-1.5 text-[13px]"
                                rows={Math.min(8, draft.split('\n').length)}
                                maxLength={2000}
                                value={draft}
                                autoFocus
                                onFocus={event => event.target.setSelectionRange(event.target.value.length, event.target.value.length)}
                                onChange={event => setDraft(event.target.value)}
                                onKeyDown={event => {
                                    if (event.key === 'Escape') {
                                        event.stopPropagation();
                                        cancelEdit();
                                    }

                                    if (event.key === 'Enter' && ! event.shiftKey) {
                                        event.preventDefault();
                                        saveEdit();
                                    }
                                }}
                            />
                            <span className="flex items-center gap-2">
                                <span className="mr-1 font-mono text-[10px] text-ink-dim">Enter salva · Esc cancela</span>
                                <button className="btn-ghost px-2.5 py-1 text-[12px]" type="button" onClick={cancelEdit}>Cancelar</button>
                                <button className="btn-primary px-3 py-1 text-[12px] font-semibold" type="button" disabled={draft.trim() === '' || draft === message.body} onClick={saveEdit}>Salvar</button>
                            </span>
                        </span>
                    )
                    : <p className={`bubble mt-1 whitespace-pre-wrap break-words ${mine ? 'bubble-mine' : ''}`}>{message.body}</p>}
            </div>

            {! mine && actions}

        </div>
    );
});
