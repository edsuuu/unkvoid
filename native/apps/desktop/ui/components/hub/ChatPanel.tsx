import { useLayoutEffect, useRef, useState } from 'react';

import { Permissions } from '../../core/Permissions.ts';
import { Icon } from '../common/Icon.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { MessageRow } from './MessageRow.tsx';

const STICK_PX = 80;
const LOAD_OLDER_PX = 60;
const COMPOSER_MAX_PX = 160;

export function ChatPanel({ onClose = null }: { onClose?: (() => void) | null }) {
    const hub = useApp().hub;
    const chat = hub.chat;
    const { channel, tree } = useStore(hub.store);
    const { messages, loading, loadingOlder, failed, replyTo, newFrom } = useStore(chat.store);
    const [draft, setDraft] = useState('');
    const [draftChannelId, setDraftChannelId] = useState(channel?.id);
    const list = useRef<HTMLDivElement>(null);
    const composer = useRef<HTMLTextAreaElement>(null);
    const stick = useRef(true);
    const prependFrom = useRef<number | null>(null);

    if (draftChannelId !== channel?.id) {
        setDraftChannelId(channel?.id);
        setDraft('');
    }

    useLayoutEffect(() => {
        const element = list.current;

        if (! element) {
            return;
        }

        if (prependFrom.current !== null) {
            element.scrollTop += element.scrollHeight - prependFrom.current;
            prependFrom.current = null;

            return;
        }

        if (stick.current) {
            element.scrollTop = element.scrollHeight;

            return;
        }

        if (messages.length > 0) {
            chat.markUnreadFrom(messages[messages.length - 1].id);
        }
    }, [messages, chat]);

    useLayoutEffect(() => {
        stick.current = true;

        if (composer.current) {
            composer.current.style.height = '';
        }
    }, [channel?.id]);

    if (! channel) {
        return (
            <section className="glass flex min-h-0 flex-1 items-center justify-center p-6 text-[13px] text-ink-dim">
                {tree?.channels.some(item => item.type === 'text') ? 'Escolha um canal de texto à esquerda.' : 'Este servidor ainda não tem canal de texto.'}
            </section>
        );
    }

    const canSend = Permissions.has(channel.permissions, Permissions.SEND_MESSAGES);

    const onScroll = () => {
        const element = list.current;

        if (! element) {
            return;
        }

        stick.current = element.scrollHeight - element.scrollTop - element.clientHeight < STICK_PX;

        if (element.scrollTop < LOAD_OLDER_PX && ! loadingOlder && messages.length) {
            prependFrom.current = element.scrollHeight;
            void chat.loadOlder().then(loaded => {
                if (! loaded) {
                    prependFrom.current = null;
                }
            });
        }
    };

    const grow = () => {
        const element = composer.current;

        if (! element) {
            return;
        }

        element.style.height = 'auto';
        element.style.height = `${Math.min(element.scrollHeight, COMPOSER_MAX_PX)}px`;
    };

    const send = async () => {
        const body = draft;

        if (body.trim() === '') {
            return;
        }

        setDraft('');
        stick.current = true;
        requestAnimationFrame(grow);

        if (! await chat.send(body)) {
            setDraft(current => (current === '' ? body : current));
        }
    };

    return (
        <section className="glass relative flex min-h-0 flex-1 animate-fade-in flex-col gap-3 p-4">
            <div className="flex items-center gap-2 border-b border-line pb-3">
                <span className="font-mono text-[14px] text-lilac-2">#</span>
                <span className="text-[14px] font-semibold">{channel.name}</span>
                {channel.topic && <span className="min-w-0 flex-1 truncate border-l border-line pl-3 text-[12.5px] text-ink-soft">{channel.topic}</span>}
                <span className="flex-1" />
                {! onClose && (
                    <button className="btn-icon size-7 rounded-[9px]" type="button" title="Mostrar ou esconder os membros" onClick={() => hub.toggleMembers()}>
                        <Icon name="users" size={14} />
                    </button>
                )}
                {hub.can(Permissions.MANAGE_CHANNELS) && (
                    <button className="btn-icon size-7 rounded-[9px]" type="button" title="Editar canal" onClick={() => hub.openModal({ type: 'channel', channel, channelType: channel.type })}>
                        <Icon name="edit" size={13} />
                    </button>
                )}
                {onClose && (
                    <button className="cursor-pointer p-1 text-ink-dim hover:text-ink-strong" type="button" title="Fechar o chat" onClick={onClose}>
                        <Icon name="close" size={14} />
                    </button>
                )}
            </div>

            <div ref={list} className="scroll-thin flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto pr-1" onScroll={onScroll}>
                {loadingOlder && <p className="pointer-events-none absolute top-[62px] left-1/2 z-10 -translate-x-1/2 rounded-full border border-line bg-[rgba(16,13,26,0.92)] px-3 py-1 font-mono text-[10.5px] text-ink-dim">carregando mensagens antigas…</p>}

                {loading && [0, 1, 2].map(index => (
                    <div key={index} className={`flex gap-2.5 ${index === 1 ? 'flex-row-reverse' : ''}`}>
                        <span className="skeleton size-7 flex-none rounded-full" />
                        <span className="skeleton h-12 w-2/5 rounded-xl" />
                    </div>
                ))}

                {! loading && messages.length === 0 && failed && (
                    <div className="m-auto flex flex-col items-center gap-2 text-center">
                        <p className="text-[13px] text-danger">Não deu para carregar as mensagens de #{channel.name}.</p>
                        <button className="btn-ghost" type="button" onClick={() => void hub.attempt(() => chat.open(channel))}>Tentar de novo</button>
                    </div>
                )}

                {! loading && messages.length === 0 && ! failed && (
                    <p className="m-auto text-center text-[13px] text-ink-dim">Nenhuma mensagem ainda em #{channel.name}.</p>
                )}

                {messages.map(message => (
                    <MessageRow key={message.id} message={message} mine={chat.isMine(message)} canDelete={chat.canDelete(message)} unreadMark={message.id === newFrom} />
                ))}
            </div>

            {replyTo && (
                <div className="flex items-center gap-2 rounded-[10px] border border-line-strong bg-row px-3 py-1.5 text-[12px]">
                    <Icon name="arrowLeft" size={12} className="rotate-90 text-ink-dim" />
                    <span className="text-ink-dim">Respondendo</span>
                    <span className="flex-none font-medium">{replyTo.user.name}</span>
                    <span className="min-w-0 flex-1 truncate text-ink-dim">{replyTo.body}</span>
                    <button className="cursor-pointer p-0.5 text-ink-dim hover:text-ink-strong" type="button" title="Cancelar a resposta" onClick={() => chat.reply(null)}>
                        <Icon name="close" size={13} />
                    </button>
                </div>
            )}

            <form className="flex items-end gap-2" onSubmit={event => { event.preventDefault(); void send(); }}>
                <textarea
                    ref={composer}
                    className="field scroll-thin min-w-0 flex-1 resize-none py-2.5 text-[13px]"
                    rows={1}
                    maxLength={2000}
                    value={draft}
                    disabled={! canSend}
                    placeholder={! canSend ? 'Você não pode enviar mensagens neste canal' : replyTo ? `Respondendo ${replyTo.user.name}` : `Escreva em #${channel.name}`}
                    onChange={event => { setDraft(event.target.value); grow(); }}
                    onKeyDown={event => {
                        if (event.key === 'Escape' && replyTo) {
                            event.preventDefault();
                            chat.reply(null);
                        }

                        if (event.key === 'Enter' && ! event.shiftKey) {
                            event.preventDefault();
                            void send();
                        }
                    }}
                />
                <button className="btn-primary px-4 py-2.5 text-[13px]" type="submit" disabled={! canSend || draft.trim() === ''}>Enviar</button>
            </form>
        </section>
    );
}
