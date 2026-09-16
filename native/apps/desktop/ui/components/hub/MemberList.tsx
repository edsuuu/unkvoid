import { Members } from '../../core/Members.ts';
import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function MemberList() {
    const hub = useApp().hub;
    const { tree, online } = useStore(hub.store);

    if (! tree) {
        return null;
    }

    const groups = Members.group(tree, online);

    return (
        <aside className="glass scroll-thin flex w-[208px] flex-none animate-fade-in flex-col gap-4 overflow-y-auto p-3.5">
            {groups.map(group => (
                <div key={group.key}>
                    <p className="label-mono mb-2" style={group.color ? { color: group.color } : undefined}>
                        {group.label} — {group.members.length}
                    </p>

                    <div className="flex flex-col gap-0.5">
                        {group.members.map(member => {
                            const offline = group.key === Members.OFFLINE_KEY;

                            return (
                                <button
                                    key={member.user_id}
                                    className={`row-item w-full cursor-pointer text-left ${offline ? 'opacity-55' : ''}`}
                                    type="button"
                                    onClick={event => hub.openMemberMenu(member, event.clientX, event.clientY)}
                                    onContextMenu={event => { event.preventDefault(); hub.openMemberMenu(member, event.clientX, event.clientY); }}
                                >
                                    <Avatar name={Members.displayName(member)} url={member.avatar_url} size={24} mine={member.user_id === hub.user?.id} />
                                    <span className="min-w-0 flex-1 truncate text-[12.5px]" style={group.color && ! offline ? { color: group.color } : undefined}>
                                        {Members.displayName(member)}
                                    </span>
                                    {member.is_owner && <span className="flex-none text-lilac-2" title="Dono do servidor"><Icon name="crown" size={12} /></span>}
                                </button>
                            );
                        })}
                    </div>
                </div>
            ))}
        </aside>
    );
}
