import { beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { App } from '../../ui/core/App.ts';
import type { Chat } from '../../ui/core/Chat.ts';
import { Direct } from '../../ui/core/Direct.ts';
import { ImageShrinker } from '../../ui/core/ImageShrinker.ts';
import type { DirectConversation, DirectMessage, Person } from '../../ui/core/Models.ts';
import { Permissions } from '../../ui/core/Permissions.ts';

const MEGABYTE = 1024 * 1024;
const picture = (name: string, type: string, bytes: number) => new File([new Uint8Array(bytes)], name, { type });

describe('a imagem é reduzida antes de sair do app', () => {
    const encodes: { width: number; height: number; type: string; quality: number }[] = [];
    let answer: (type: string, quality: number, width: number) => Blob;

    beforeAll(() => {
        ImageShrinker.decode = async () => ({ width: 5120, height: 2880 });
        ImageShrinker.encode = async (_image, width, height, type, quality) => {
            encodes.push({ width, height, type, quality });

            return answer(type, quality, width);
        };
    });

    beforeEach(() => {
        encodes.length = 0;
    });

    it('tipo aceito e até 2 MB vai intacto, GIF inclusive; GIF maior é recusado porque reexportar mata a animação', async () => {
        const small = picture('print.png', 'image/png', 1024);
        const animated = picture('meme.gif', 'image/gif', 2 * MEGABYTE);

        expect(await ImageShrinker.fit(small)).toBe(small);
        expect(await ImageShrinker.fit(animated)).toBe(animated);
        await expect(ImageShrinker.fit(picture('meme.gif', 'image/gif', 2 * MEGABYTE + 1))).rejects.toThrow('GIF acima de 2 MB');
        await expect(ImageShrinker.fit(picture('notas.txt', 'text/plain', 10))).rejects.toThrow('só dá para anexar imagem');
        expect(encodes, 'nada disso passa pelo canvas').toEqual([]);
    });

    it('o lado maior cai para 2560 sem esticar, e o WebP que cabe é o que vai', async () => {
        answer = type => new Blob([new Uint8Array(MEGABYTE)], { type });

        const fitted = await ImageShrinker.fit(picture('foto.bmp', 'image/bmp', 100));

        expect(encodes).toEqual([{ width: 2560, height: 1440, type: 'image/webp', quality: 0.9 }]);
        expect(fitted.type).toBe('image/webp');
        expect(fitted.name, 'a extensão acompanha o tipo que saiu').toBe('foto.webp');
    });

    it('o WebKit que devolve PNG calado no lugar de WebP cai para JPEG, e desce os degraus até caber em 2 MB', async () => {
        answer = (type, quality, width) => (type === 'image/webp'
            ? new Blob([new Uint8Array(10)], { type: 'image/png' })
            : new Blob([new Uint8Array(width === 2560 ? 3 * MEGABYTE : MEGABYTE)], { type }));

        const fitted = await ImageShrinker.fit(picture('print.png', 'image/png', 5 * MEGABYTE));

        expect(encodes.map(encode => encode.type), 'WebP só é tentado uma vez').toEqual(['image/webp', 'image/jpeg', 'image/jpeg', 'image/jpeg']);
        expect(encodes.at(-1)).toMatchObject({ width: 1920, height: 1080, type: 'image/jpeg' });
        expect(fitted.type).toBe('image/jpeg');
        expect(fitted.name).toBe('print.jpeg');
        expect(fitted.size).toBeLessThanOrEqual(ImageShrinker.MAX_BYTES);
    });

    it('se nem o último degrau cabe, recusa em vez de mandar o que o servidor vai devolver', async () => {
        answer = type => new Blob([new Uint8Array(2 * MEGABYTE + 1)], { type });

        await expect(ImageShrinker.fit(picture('mapa.png', 'image/png', 9 * MEGABYTE))).rejects.toThrow('não deu para reduzir');
        expect(encodes.length).toBe(ImageShrinker.STEPS.length);
    });
});

describe('imagem no chat de canal', () => {
    const calls: { method: string; path: string; body: unknown }[] = [];
    const toasts: string[] = [];
    const responses = new Map<string, unknown>();
    const revoked: string[] = [];
    const quiet = { listen() { return this; }, stopListening() { return this; } };
    const channel = { id: 'text-1', name: 'geral', type: 'text', topic: null, position: 0, permissions: Permissions.ALL };
    const message = (id: number, url: string | null = null) => ({ id, channel_id: 'text-1', type: 'user', body: `m${id}`, reply_to: null, user: { id: 7, name: 'Bia' }, files: url ? [{ id: id * 10, url, mime_type: 'image/png', size: 10 }] : [] });
    let chat: Chat;
    let previews = 0;

    beforeAll(async () => {
        const app = new App();
        const hub = app.hub;

        URL.createObjectURL = () => `blob:preview-${++previews}`;
        URL.revokeObjectURL = url => revoked.push(url);
        app.toast = text => toasts.push(text);
        hub.api.request = async (method: string, path: string, body: unknown) => {
            calls.push({ method, path, body });

            const answer = responses.get(`${method} ${path}`);

            return typeof answer === 'function' ? answer(body) : answer ?? [];
        };
        hub.user = { id: 1, name: 'Edsu' };
        hub.echo = { private: () => quiet, leave() {} };
        chat = hub.chat;
        await chat.open(channel);
    });

    it('anexa no máximo 3, e a que não é imagem vira aviso sem derrubar as outras', async () => {
        await chat.attach([picture('a.png', 'image/png', 10), picture('notas.txt', 'text/plain', 10), picture('b.png', 'image/png', 10)]);

        expect(chat.store.state.images.map(image => image.file.name)).toEqual(['a.png', 'b.png']);
        expect(toasts.at(-1)).toBe('só dá para anexar imagem');

        await chat.attach([picture('c.png', 'image/png', 10), picture('d.png', 'image/png', 10)]);

        expect(chat.store.state.images.map(image => image.file.name), 'a quarta fica de fora').toEqual(['a.png', 'b.png', 'c.png']);
        expect(toasts.at(-1)).toBe('no máximo 3 imagens por mensagem');

        chat.detach(chat.store.state.images[1].id);

        expect(chat.store.state.images.map(image => image.file.name)).toEqual(['a.png', 'c.png']);
        expect(revoked, 'a miniatura tirada solta a memória dela').toEqual(['blob:preview-2']);
    });

    it('com imagem o envio é multipart com images[], body e reply_to_id; deu certo, a fila esvazia', async () => {
        responses.set('POST /api/channels/text-1/messages', message(50, 'https://bucket/a.png'));
        chat.reply(message(9));
        calls.length = 0;

        expect(await chat.send('  olha isso  ')).toBe(true);

        const form = calls[0].body as FormData;

        expect(form).toBeInstanceOf(FormData);
        expect(form.get('body')).toBe('olha isso');
        expect(form.get('reply_to_id')).toBe('9');
        expect((form.getAll('images[]') as File[]).map(file => file.name)).toEqual(['a.png', 'c.png']);
        expect(chat.store.state.images).toEqual([]);
        expect(chat.store.state.sending).toBe(false);
        expect(chat.store.state.messages.at(-1)?.files[0].url).toBe('https://bucket/a.png');
    });

    it('mensagem só com imagem é válida e vai sem o campo body; sem imagem continua sendo JSON', async () => {
        await chat.attach([picture('so-imagem.png', 'image/png', 10)]);
        calls.length = 0;

        expect(await chat.send('   ')).toBe(true);
        expect((calls[0].body as FormData).has('body')).toBe(false);
        expect((calls[0].body as FormData).getAll('images[]').length).toBe(1);

        expect(await chat.send('só texto')).toBe(true);
        expect(calls[1].body).toEqual({ body: 'só texto', reply_to_id: null });

        calls.length = 0;
        expect(await chat.send('   '), 'vazio e sem imagem não vai para a API').toBe(true);
        expect(calls).toEqual([]);
    });

    it('o 413 do pedido acima de 8 MB vira aviso de imagem grande demais, e as imagens ficam para tentar de novo', async () => {
        responses.set('POST /api/channels/text-1/messages', () => {
            throw Object.assign(new Error('o servidor respondeu 413'), { status: 413 });
        });
        await chat.attach([picture('enorme.png', 'image/png', 10)]);

        expect(await chat.send('')).toBe(false);
        expect(toasts.at(-1)).toContain('imagens grandes demais');
        expect(chat.store.state.images.length, 'nada se perde numa falha').toBe(1);
        expect(chat.store.state.sending).toBe(false);

        chat.detach(chat.store.state.images[0].id);
    });

    it('link vencido: a imagem que falha pede a página dela de novo uma vez, e a que continua quebrada não entra em laço', async () => {
        chat.append(message(60, 'https://bucket/vencido'));
        responses.set('GET /api/channels/text-1/messages?before=61', [message(60, 'https://bucket/novo')]);
        calls.length = 0;

        await chat.renewFiles(chat.store.state.messages.at(-1)!);

        expect(calls.map(call => call.path)).toEqual(['/api/channels/text-1/messages?before=61']);
        expect(chat.store.state.messages.at(-1)?.files[0].url).toBe('https://bucket/novo');

        await chat.renewFiles(chat.store.state.messages.at(-1)!);

        expect(calls.length, 'o link novo também falhou: não pede de novo').toBe(1);
    });
});

describe('lista de conversas: a última mensagem sobe, e o contador só cresce no que eu não li', () => {
    const ana: Person = { id: 2, name: 'Ana', avatar_url: null };
    const bia: Person = { id: 3, name: 'Bia', avatar_url: null };

    function message(id: number, body: string, mine: boolean, sender: Person): DirectMessage {
        return { id, body, created_at: '2026-09-16T00:00:00-03:00', edited_at: null, mine, sender };
    }

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
