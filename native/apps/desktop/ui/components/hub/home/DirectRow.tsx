import { memo, useState } from 'react';

import type { DirectMessage } from '../../../core/Models.ts';
import { Avatar } from '../../common/Avatar.tsx';
import { Icon } from '../../common/Icon.tsx';
import { useApp } from '../../useApp.ts';

const TIME_FORMAT: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit', day: '2-digit', month: '2-digit' };

const ACTION = 'flex size-7 cursor-pointer items-center justify-center rounded-lg border border-line-strong bg-row text-ink-icon transition hover:border-brand/50 hover:text-ink-strong';

export const DirectRow = memo(function DirectRow({ message }: { message: DirectMessage }) {
    const direct = useApp().hub.direct;
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
        void direct.edit(message, draft).then(saved => saved || setEditing(true));
    };

    return (
        <div className="group relative flex animate-fade-in gap-2.5 rounded-[10px] px-1 py-0.5 hover:bg-white/[0.03]">
            <Avatar name={message.sender.name} url={message.sender.avatar_url} size={30} mine={message.mine} />

            <div className="flex min-w-0 flex-1 flex-col">
                <p className="text-[11px] text-ink-dim">
                    <span className="text-[12.5px] font-semibold text-ink-body">{message.sender.name}</span>
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

            {! editing && message.mine && (
                <span className="absolute -top-2 right-2 flex items-center gap-1 rounded-[10px] border border-line-strong bg-[rgba(16,13,26,0.96)] p-0.5 opacity-0 transition group-hover:opacity-100 focus-within:opacity-100">
                    <button className={ACTION} type="button" title="Editar" onClick={() => { setDraft(message.body); setEditing(true); }}>
                        <Icon name="edit" size={13} />
                    </button>

                    <button className={`${ACTION} hover:border-danger/45 hover:text-danger`} type="button" title="Apagar" onClick={() => void direct.destroy(message)}>
                        <Icon name="trash" size={13} />
                    </button>
                </span>
            )}
        </div>
    );
});
