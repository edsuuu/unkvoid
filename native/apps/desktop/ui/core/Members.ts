import type { Member, Role, ServerTree } from './Models.ts';

export type MemberGroup = {
    key: string;
    label: string;
    color: string | null;
    members: Member[];
};

export class Members {
    static readonly OFFLINE_KEY = 'offline';

    static readonly EVERYONE_KEY = 'everyone';

    static displayName(member: Member): string {
        return member.nickname ?? member.name;
    }

    static topRole(tree: ServerTree, member: Member): Role | null {
        return tree.roles
            .filter(role => ! role.is_everyone && member.role_ids.includes(role.id))
            .sort((left, right) => right.position - left.position)[0] ?? null;
    }

    static group(tree: ServerTree, online: Set<number>): MemberGroup[] {
        const everyone = tree.roles.find(role => role.is_everyone) ?? null;
        const groups = new Map<string, MemberGroup>();
        const offline: MemberGroup = { key: Members.OFFLINE_KEY, label: 'Offline', color: null, members: [] };

        for (const member of tree.members) {
            if (! online.has(member.user_id)) {
                offline.members.push(member);

                continue;
            }

            const role = Members.topRole(tree, member);
            const key = role ? String(role.id) : Members.EVERYONE_KEY;
            const group = groups.get(key) ?? {
                key,
                label: role?.name ?? everyone?.name ?? 'Membros',
                color: role?.color ?? null,
                members: [],
            };

            group.members.push(member);
            groups.set(key, group);
        }

        const position = (group: MemberGroup) => tree.roles.find(role => String(role.id) === group.key)?.position ?? -1;
        const byName = (left: Member, right: Member) => Members.displayName(left).localeCompare(Members.displayName(right), 'pt-BR');

        const sorted = [...groups.values()].sort((left, right) => position(right) - position(left));

        for (const group of [...sorted, offline]) {
            group.members.sort(byName);
        }

        return offline.members.length > 0 ? [...sorted, offline] : sorted;
    }
}
