import { describe, expect, it } from 'vitest';

import { Direct } from '../../ui/core/Direct.ts';
import type { DirectConversation, DirectMessage, Person } from '../../ui/core/Models.ts';

const ana: Person = { id: 2, name: 'Ana', avatar_url: null };
const bia: Person = { id: 3, name: 'Bia', avatar_url: null };

function message(id: number, body: string, mine: boolean, sender: Person): DirectMessage {
    return { id, body, created_at: '2026-09-16T00:00:00-03:00', edited_at: null, mine, sender };
}

describe('lista de conversas: a última mensagem sobe, e o contador só cresce no que eu não li', () => {
    it('conversa nova entra no topo com uma não lida', () => {
        const list = Direct.bump([], ana, message(1, 'oi', false, ana), false);

        expect(list).toHaveLength(1);
        expect(list[0].user.id).toBe(ana.id);
        expect(list[0].last?.body).toBe('oi');
        expect(list[0].unread).toBe(1);
    });

    it('mensagem nova tira a conversa do fundo e põe no topo, somando as não lidas', () => {
        const antiga: DirectConversation[] = [
            { user: bia, last: { id: 9, body: 'e aí', created_at: '', mine: false }, unread: 0 },
            { user: ana, last: { id: 5, body: 'oi', created_at: '', mine: false }, unread: 1 },
        ];

        const list = Direct.bump(antiga, ana, message(10, 'tudo bem?', false, ana), false);

        expect(list.map(item => item.user.id), 'a conversa que recebeu vai para o topo').toEqual([ana.id, bia.id]);
        expect(list[0].unread, 'a segunda não lida soma com a primeira').toBe(2);
        expect(list, 'nenhuma conversa é duplicada').toHaveLength(2);
    });

    it('o que eu mesmo mando nunca conta como não lido, e abrir a conversa zera', () => {
        const comNaoLidas: DirectConversation[] = [{ user: ana, last: null, unread: 3 }];

        expect(Direct.bump(comNaoLidas, ana, message(11, 'respondi', true, bia), false)[0].unread).toBe(0);
        expect(Direct.bump(comNaoLidas, ana, message(12, 'chegou', false, ana), true)[0].unread, 'com a conversa aberta, já nasce lida').toBe(0);
    });
});
