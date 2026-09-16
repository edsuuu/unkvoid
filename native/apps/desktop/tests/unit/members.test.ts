import { describe, expect, it } from 'vitest';

import { Members } from '../../ui/core/Members.ts';
import type { Member, Role, ServerTree } from '../../ui/core/Models.ts';

function role(id: number, name: string, position: number, everyone = false): Role {
    return { id, name, color: null, position, permissions: 0, is_everyone: everyone };
}

function member(userId: number, name: string, roleIds: number[], nickname: string | null = null): Member {
    return { user_id: userId, name, avatar_url: null, nickname, role_ids: roleIds, server_mute: false, server_deaf: false, is_owner: false };
}

const tree = {
    roles: [role(1, '@everyone', 0, true), role(2, 'Moderador', 10), role(3, 'Admin', 20)],
    members: [
        member(10, 'Zeca', [2]),
        member(11, 'Ana', [3, 2]),
        member(12, 'Bia', []),
        member(13, 'Caio', [3]),
        member(14, 'Dora', [2]),
    ],
} as ServerTree;

describe('lista de membros: cargo mais alto manda, e quem está fora aparece por último', () => {
    it('agrupa pelo cargo de maior posição, ordena os grupos de cima para baixo e os nomes dentro de cada um', () => {
        const groups = Members.group(tree, new Set([10, 11, 12, 13, 14]));

        expect(groups.map(group => group.label)).toEqual(['Admin', 'Moderador', '@everyone']);
        expect(groups[0].members.map(item => item.name), 'quem tem Admin e Moderador entra só no Admin').toEqual(['Ana', 'Caio']);
        expect(groups[1].members.map(item => item.name)).toEqual(['Dora', 'Zeca']);
        expect(groups[2].members.map(item => item.name), 'sem cargo cai no @everyone').toEqual(['Bia']);
    });

    it('quem não está online sai do grupo do cargo e vira o último grupo', () => {
        const groups = Members.group(tree, new Set([11]));

        expect(groups.map(group => group.label)).toEqual(['Admin', 'Offline']);
        expect(groups[1].members.map(item => item.name)).toEqual(['Bia', 'Caio', 'Dora', 'Zeca']);
    });

    it('sem ninguém offline o grupo Offline não aparece, e o apelido é quem ordena', () => {
        const apelidado = { ...tree, members: [member(20, 'Zeca', [2], 'Alfa'), member(21, 'Ana', [2])] } as ServerTree;
        const groups = Members.group(apelidado, new Set([20, 21]));

        expect(groups.map(group => group.label)).toEqual(['Moderador']);
        expect(groups[0].members.map(item => Members.displayName(item))).toEqual(['Alfa', 'Ana']);
    });
});
