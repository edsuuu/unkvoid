import { Members } from '../../../../core/Members.ts';
import { Avatar } from '../../../common/Avatar.tsx';
import { Icon } from '../../../common/Icon.tsx';
import { useApp } from '../../../useApp.ts';
import { useStore } from '../../../useStore.ts';

export function MembersTab() {
    const hub = useApp().hub;
    const { tree, user, online } = useStore(hub.store);

    if (! tree || ! user) {
        return null;
    }

    const everyone = tree.roles.find(role => role.is_everyone) ?? null;
    const members = [...tree.members].sort((left, right) => Members.displayName(left).localeCompare(Members.displayName(right), 'pt-BR'));

    return (
        <>
            <div className="mb-2 flex items-center justify-between">
                <span className="label-mono">Membros</span>
                <span className="font-mono text-[10px] text-ink-dim">{members.length}</span>
            </div>

            <div className="flex flex-col gap-1.5">
                {members.map(member => {
                    const topRole = Members.topRole(tree, member);
                    const me = member.user_id === user.id;

                    return (
                        <button
                            key={member.user_id}
                            className="row-item w-full cursor-pointer text-left"
                            type="button"
                            onClick={event => hub.openMemberMenu(member, event.clientX, event.clientY)}
                            onContextMenu={event => { event.preventDefault(); hub.openMemberMenu(member, event.clientX, event.clientY); }}
                        >
                            <Avatar name={Members.displayName(member)} url={member.avatar_url} size={26} mine={me} status={online.has(member.user_id) ? 'online' : 'offline'} />
                            <span className={`min-w-0 flex-1 truncate text-[13px] ${me ? 'text-ink-strong' : 'text-ink-icon'}`}>
                                {Members.displayName(member)}
                                {me && <span className="ml-1.5 font-mono text-[9.5px] text-ink-dim">você</span>}
                            </span>
                            {member.server_mute && <span className="text-danger" title="Mutado no servidor"><Icon name="micOff" size={13} /></span>}
                            {member.server_deaf && <span className="text-danger" title="Ensurdecido no servidor"><Icon name="headphonesOff" size={13} /></span>}
                            <span className="size-[7px] rounded-full" style={{ background: (topRole ?? everyone)?.color ?? 'var(--color-ink-dim)' }} />
                            <span className={`font-mono text-[9.5px] ${member.is_owner ? 'text-lilac-2' : 'text-ink-dim'}`}>{member.is_owner ? 'dono' : topRole?.name ?? everyone?.name ?? '@everyone'}</span>
                            <Icon name="dots" size={13} className="text-ink-dim" />
                        </button>
                    );
                })}
            </div>
        </>
    );
}
