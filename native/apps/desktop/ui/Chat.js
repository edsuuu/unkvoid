import { Permissions } from './Permissions.js';

const el = id => document.getElementById(id);

/** Um canal de texto: a lista de mensagens, o compositor e o canal privado do Reverb. */
export class Chat {
    /** Quão perto do topo a rolagem precisa chegar para buscar mensagens mais antigas. */
    static LOAD_OLDER_PX = 60;

    /** Acima disto as mais antigas saem da tela; rolar para cima as busca de novo. */
    static MAX_ROWS = 500;

    /** Até onde o compositor cresce sozinho antes de rolar por dentro. */
    static COMPOSER_MAX_PX = 160;

    constructor(hub) {
        this.hub = hub;
        this.channel = null;
        this.subscription = null;
        this.loading = false;
        this.exhausted = false;

        /** id → linha desenhada: editar e apagar acham a linha sem varrer a lista. */
        this.rows = new Map();

        el('composer').onsubmit = event => {
            event.preventDefault();
            void this.send();
        };
        el('composer-input').onkeydown = event => {
            if (event.key === 'Enter' && ! event.shiftKey) {
                event.preventDefault();
                void this.send();
            }
        };
        el('composer-input').oninput = () => this.growComposer();
        el('message-list').onscroll = () => {
            if (el('message-list').scrollTop < Chat.LOAD_OLDER_PX) {
                void this.loadOlder();
            }
        };
    }

    /** Assina ANTES de buscar o histórico: nada chega no vão entre um e outro. */
    async open(channel) {
        this.close();
        this.channel = channel;
        this.exhausted = false;
        this.rows.clear();
        el('message-list').innerHTML = '';

        const canSend = Permissions.has(channel.permissions, Permissions.SEND_MESSAGES);

        el('composer-input').disabled = ! canSend;
        el('composer-input').placeholder = canSend ? 'Mensagem' : 'Você não pode enviar mensagens neste canal';

        this.subscription = this.hub.echo.private(`channel.${channel.id}`);
        this.hub.listen(this.subscription, 'MessageSent', ({ message }) => {
            this.append(message);
            this.scrollToEnd();
        });
        this.hub.listen(this.subscription, 'MessageUpdated', ({ message }) => this.append(message));
        this.hub.listen(this.subscription, 'MessageDeleted', ({ id }) => this.remove(id));

        const messages = await this.hub.api.get(`/api/channels/${channel.id}/messages`);

        if (this.channel !== channel) {
            return;
        }

        // Do fim para o começo, por cima: o que chegou ao vivo enquanto o histórico
        // vinha já está embaixo, e é lá que fica.
        for (const message of messages.reverse()) {
            this.append(message, true);
        }

        this.scrollToEnd();
    }

    close() {
        if (this.channel) {
            this.hub.echo.leave(`channel.${this.channel.id}`);
        }

        this.channel = null;
        this.subscription = null;
    }

    row(id) {
        return this.rows.get(id) ?? null;
    }

    remove(id) {
        this.row(id)?.remove();
        this.rows.delete(id);
    }

    scrollToEnd() {
        el('message-list').scrollTop = el('message-list').scrollHeight;
    }

    growComposer() {
        const input = el('composer-input');

        input.style.height = 'auto';
        input.style.height = `${Math.min(input.scrollHeight, Chat.COMPOSER_MAX_PX)}px`;
    }

    /** As 50 anteriores à mais antiga da tela, sem mexer no que a pessoa está lendo. */
    async loadOlder() {
        const first = el('message-list').firstElementChild;

        if (this.loading || this.exhausted || ! first || ! this.channel) {
            return;
        }

        this.loading = true;

        try {
            const older = await this.hub.api.get(`/api/channels/${this.channel.id}/messages?before=${first.dataset.message}`);
            const before = el('message-list').scrollHeight;

            this.exhausted = older.length === 0;

            for (const message of older.reverse()) {
                this.append(message, true);
            }

            el('message-list').scrollTop += el('message-list').scrollHeight - before;
        } catch (failure) {
            this.hub.app.toast(`não deu para carregar mensagens antigas: ${failure.message}`, true);
        } finally {
            this.loading = false;
        }
    }

    /** Desenha, ou redesenha no lugar se o id já está na tela (edição, eco do próprio envio). */
    append(message, prepend = false) {
        if (message.channel_id !== this.channel?.id) {
            return;
        }

        const existing = this.row(message.id);
        const row = existing ?? document.createElement('div');
        const mine = message.user.id === this.hub.user.id;
        const canDelete = mine || Permissions.has(this.channel.permissions, Permissions.MANAGE_MESSAGES);
        const when = new Date(message.created_at).toLocaleString('pt-BR', { hour: '2-digit', minute: '2-digit', day: '2-digit', month: '2-digit' });

        row.dataset.message = message.id;
        row.className = `group flex gap-2.5 px-2 py-1.5 ${mine ? 'flex-row-reverse' : ''}`;
        row.innerHTML = `<span class="avatar mt-0.5 size-9 shrink-0 text-sm ${mine ? '' : 'avatar-flat'}"></span>`
            + `<div class="min-w-0 flex-1 ${mine ? 'text-right' : ''}">`
            + '<p class="text-[11.5px] text-ink-dim"><span data-author></span> <span data-time></span></p>'
            + `<p class="bubble mt-1 inline-block whitespace-pre-wrap break-words text-[13px] ${mine ? 'bubble-mine' : ''}" data-body></p>`
            + '</div>'
            + '<span class="hidden shrink-0 gap-1 self-start group-hover:flex focus-within:flex">'
            + '<button class="cursor-pointer rounded px-1.5 text-xs text-ink-soft hover:text-white" data-edit type="button" hidden>Editar</button>'
            + '<button class="cursor-pointer rounded px-1.5 text-xs text-ink-soft hover:text-danger" data-delete type="button" hidden>Apagar</button>'
            + '</span>';

        row.querySelector('span').textContent = (message.user.name ?? '?').slice(0, 1).toUpperCase();
        row.querySelector('[data-author]').textContent = message.user.name;
        row.querySelector('[data-time]').textContent = message.edited_at ? `${when} (editada)` : when;
        row.querySelector('[data-body]').textContent = message.body;
        row.querySelector('[data-edit]').hidden = ! mine;
        row.querySelector('[data-delete]').hidden = ! canDelete;
        row.querySelector('[data-edit]').onclick = () => this.edit(row, message);
        row.querySelector('[data-delete]').onclick = () => {
            if (confirm('Apagar esta mensagem?')) {
                void this.hub.attempt(() => this.hub.api.delete(`/api/messages/${message.id}`));
            }
        };

        if (existing) {
            return;
        }

        this.rows.set(message.id, row);
        el('message-list')[prepend ? 'prepend' : 'append'](row);

        // Um canal aberto por dias acumularia milhares de nós: as mais antigas saem, e
        // rolar para cima as busca de novo como se nunca tivessem sido carregadas.
        while (! prepend && el('message-list').childElementCount > Chat.MAX_ROWS) {
            this.remove(el('message-list').firstElementChild.dataset.message);
            this.exhausted = false;
        }
    }

    /** A edição acontece no lugar: Enter salva, Esc desiste. */
    edit(row, message) {
        const body = row.querySelector('[data-body]');
        const input = document.createElement('textarea');

        input.className = 'field w-full resize-none px-2 py-1';
        input.value = message.body;
        input.rows = Math.min(8, message.body.split('\n').length);
        body.replaceWith(input);
        input.focus();

        input.onkeydown = event => {
            if (event.key === 'Escape') {
                input.replaceWith(body);
            }

            if (event.key === 'Enter' && ! event.shiftKey) {
                event.preventDefault();
                input.replaceWith(body);
                this.hub.attempt(async () => this.append(await this.hub.api.patch(`/api/messages/${message.id}`, { body: input.value.trim() })));
            }
        };
    }

    async send() {
        const input = el('composer-input');
        const body = input.value.trim();

        if (! body || ! this.channel) {
            return;
        }

        input.value = '';
        input.style.height = '';

        const sent = await this.hub.attempt(async () => {
            this.append(await this.hub.api.post(`/api/channels/${this.channel.id}/messages`, { body }));
            this.scrollToEnd();

            return true;
        });

        // Falhou: o texto volta para a caixa em vez de sumir com o aviso.
        if (! sent && ! input.value) {
            input.value = body;
            this.growComposer();
        }
    }
}
