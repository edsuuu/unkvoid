import { memo, useState } from 'react';

import type { Message } from '../../core/Models.ts';
import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { useApp } from '../useApp.ts';

const TIME_FORMAT: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit', day: '2-digit', month: '2-digit' };

const ACTION = 'flex size-7 cursor-pointer items-center justify-center rounded-lg border border-line-strong bg-row text-ink-icon transition hover:border-brand/50 hover:text-ink-strong';

type MessageRowProps = {
    message: Message;
    mine: boolean;
    canDelete: boolean;
    unreadMark: boolean;
};

export const MessageRow = memo(function MessageRow({ message, mine, canDelete, unreadMark }: MessageRowProps) {
    const chat = useApp().hub.chat;
    const [editing, setEditing] = useState(false);
    const [draft, setDraft] = useState(message.body);
    const when = new Date(message.created_at).toLocaleString('pt-BR', TIME_FORMAT);

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

    const divider = unreadMark && (
        <div className="flex items-center gap-2 py-1">
            <span className="h-px flex-1 bg-danger/60" />
            <span className="rounded-full bg-danger px-2 py-0.5 text-[9.5px] font-semibold tracking-wide text-white uppercase">Novas mensagens</span>
        </div>
    );

    if (message.type === 'join') {
        return (
            <>
                {divider}
                <p className="flex animate-fade-in items-center justify-center gap-2 py-0.5 text-[12px] text-ink-dim">
                    <Icon name="users" size={13} />
                    <span><span className="text-ink-icon">@{message.user.name}</span> chegou no servidor</span>
                    <span className="font-mono text-[9.5px]">{when}</span>
                </p>
            </>
        );
    }

    return (
        <>
            {divider}
            <div className="group relative flex animate-fade-in gap-2.5 rounded-[10px] px-1 py-0.5 hover:bg-white/[0.03]">
                <Avatar name={message.user.name} url={message.user.avatar_url} size={30} mine={mine} />

                <div className="flex min-w-0 flex-1 flex-col">
                    {message.reply_to && (
                        <p className="mb-0.5 flex min-w-0 items-center gap-1.5 text-[11px] text-ink-dim">
                            <Icon name="arrowLeft" size={11} className="rotate-90" />
                            <span className="flex-none text-ink-icon">{message.reply_to.name}</span>
                            <span className="min-w-0 truncate">{message.reply_to.body}</span>
                        </p>
                    )}

                    <p className="text-[11px] text-ink-dim">
                        <span className="text-[12.5px] font-semibold text-ink-body">{message.user.name}</span>
                        <span className="ml-1.5">{when}</span>
                        {message.edited_at && <span className="ml-1.5 rounded-full bg-row px-1.5 py-0.5 text-[10px] text-ink-icon">editado</span>}
                    </p>

                    {editing
                        ? (
                            <span className="mt-1 flex w-full flex-col items-start gap-1.5">
                                <textarea
                                    className="field w-full resize-none px-2.5 py-1.5 text-[13px]"
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
                                    <button className="btn-primary px-3 py-1 text-[12px] font-semibold" type="button" disabled={draft.trim() === '' || draft === message.body} onClick={saveEdit}>Salvar</button>
                                    <button className="btn-ghost px-2.5 py-1 text-[12px]" type="button" onClick={cancelEdit}>Cancelar</button>
                                    <span className="font-mono text-[10px] text-ink-dim">Enter salva · Esc cancela</span>
                                </span>
                            </span>
                        )
                        : <p className="message-text mt-0.5">{message.body}</p>}
                </div>

                {! editing && (
                    <span className="absolute -top-2 right-2 flex items-center gap-1 rounded-[10px] border border-line-strong bg-[rgba(16,13,26,0.96)] p-0.5 opacity-0 transition group-hover:opacity-100 focus-within:opacity-100">
                        <button className={ACTION} type="button" title="Responder" onClick={() => chat.reply(message)}>
                            <Icon name="arrowLeft" size={14} className="rotate-90" />
                        </button>

                        {mine && (
                            <button className={ACTION} type="button" title="Editar" onClick={() => { setDraft(message.body); setEditing(true); }}>
                                <Icon name="edit" size={13} />
                            </button>
                        )}

                        {canDelete && (
                            <button className={`${ACTION} hover:border-danger/45 hover:text-danger`} type="button" title="Apagar" onClick={() => void chat.destroy(message)}>
                                <Icon name="trash" size={13} />
                            </button>
                        )}
                    </span>
                )}
            </div>
        </>
    );
});
