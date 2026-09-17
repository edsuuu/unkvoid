import { Avatar } from '../../common/Avatar.tsx';
import { Icon } from '../../common/Icon.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';
import { VoicePanel } from '../VoicePanel.tsx';

export function DirectColumn() {
    const hub = useApp().hub;
    const { homeTab } = useStore(hub.store);
    const { conversations, person, failed } = useStore(hub.direct.store);
    const friends = useStore(hub.friends.store);
    const pending = friends.list.filter(item => item.status === 'pending' && item.addressee.id === hub.user?.id).length;

    return (
        <aside className="flex w-[260px] flex-none flex-col gap-3">
            <div className="glass flex flex-col gap-1.5 p-3">
                <button
                    className={`row-item w-full cursor-pointer text-left ${homeTab === 'servers' && ! person ? 'row-item-on' : ''}`}
                    type="button"
                    onClick={() => hub.showHomeTab('servers')}
                >
                    <Icon name="home" size={15} />
                    <span className="flex-1 text-[13px]">Salas</span>
                </button>

                <button
                    className={`row-item w-full cursor-pointer text-left ${homeTab === 'friends' && ! person ? 'row-item-on' : ''}`}
                    type="button"
                    onClick={() => hub.showHomeTab('friends')}
                >
                    <Icon name="users" size={15} />
                    <span className="flex-1 text-[13px]">Amigos</span>
                    {pending > 0 && <span className="flex-none rounded-full bg-danger px-1.5 py-0.5 text-[10px] font-semibold text-white">{pending}</span>}
                </button>
            </div>

            <div className="glass scroll-thin flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto p-3">
                <p className="label-mono mb-1">Mensagens diretas</p>

                {failed && (
                    <div className="flex flex-col items-center gap-2 py-4 text-center">
                        <p className="text-[12px] text-danger">Não deu para carregar as conversas.</p>
                        <button className="btn-ghost px-2.5 py-1.5 text-[12px]" type="button" onClick={() => void hub.direct.loadConversations()}>Tentar de novo</button>
                    </div>
                )}

                {! failed && conversations.length === 0 && <p className="py-4 text-center text-[12px] text-ink-dim">Nenhuma conversa ainda.</p>}

                {conversations.map(conversation => (
                    <button
                        key={conversation.user.id}
                        className={`row-item w-full cursor-pointer text-left ${person?.id === conversation.user.id ? 'row-item-on' : ''}`}
                        type="button"
                        onClick={() => void hub.direct.open(conversation.user)}
                    >
                        <Avatar name={conversation.user.name} url={conversation.user.avatar_url} size={28} />
                        <span className="min-w-0 flex-1">
                            <span className="block truncate text-[13px] font-medium">{conversation.user.name}</span>
                            {conversation.last && (
                                <span className="block truncate text-[11.5px] text-ink-dim">
                                    {conversation.last.mine ? 'você: ' : ''}{conversation.last.body}
                                </span>
                            )}
                        </span>
                        {conversation.unread > 0 && <span className="flex-none rounded-full bg-danger px-1.5 py-0.5 text-[10px] font-semibold text-white">{conversation.unread}</span>}
                    </button>
                ))}
            </div>

            <VoicePanel />
        </aside>
    );
}
