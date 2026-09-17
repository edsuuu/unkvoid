import { Failure } from './Failure.ts';
import type { Hub } from './Hub.ts';
import type { Channel, Message } from './Models.ts';
import { Permissions } from './Permissions.ts';
import { Store } from './Store.ts';

export type ChatState = {
    channel: Channel | null;
    messages: Message[];
    loading: boolean;
    loadingOlder: boolean;
    exhausted: boolean;
    failed: boolean;
    replyTo: Message | null;
    newFrom: number | null;
};

export class Chat {
    static readonly MAX_ROWS = 500;

    readonly hub: Hub;
    channel: Channel | null = null;
    readonly store: Store<ChatState>;

    constructor(hub: Hub) {
        this.hub = hub;
        this.store = new Store<ChatState>({ channel: null, messages: [], loading: false, loadingOlder: false, exhausted: false, failed: false, replyTo: null, newFrom: null });
    }

    async open(channel: Channel): Promise<void> {
        this.close();
        this.channel = channel;
        this.store.replace({ channel, messages: [], loading: true, loadingOlder: false, exhausted: false, failed: false, replyTo: null, newFrom: null });

        const subscription = this.hub.echo!.private(`channel.${channel.id}`);

        this.hub.listen<{ message: Message }>(subscription, 'MessageSent', ({ message }) => {
            if (message.user.id !== this.hub.user?.id) {
                this.hub.app.sounds.message();
            }

            this.append(message);
        });
        this.hub.listen<{ message: Message }>(subscription, 'MessageUpdated', ({ message }) => this.append(message));
        this.hub.listen<{ id: number }>(subscription, 'MessageDeleted', ({ id }) => this.remove(id));

        let history: Message[] = [];

        try {
            history = await this.hub.api.get<Message[]>(`/api/channels/${channel.id}/messages`);
        } catch (failure) {
            if (this.channel === channel) {
                this.store.set({ failed: true });
            }

            throw failure;
        } finally {
            if (this.channel === channel) {
                this.store.set({ loading: false });
            }
        }

        if (this.channel !== channel) {
            return;
        }

        const known = new Set(this.store.state.messages.map(message => message.id));

        this.store.set(state => ({ messages: [...history.filter(message => ! known.has(message.id)), ...state.messages] }));
    }

    close(): void {
        if (this.channel) {
            this.hub.echo?.leave(`channel.${this.channel.id}`);
        }

        this.channel = null;
        this.store.replace({ channel: null, messages: [], loading: false, loadingOlder: false, exhausted: false, failed: false, replyTo: null, newFrom: null });
    }

    async loadOlder(): Promise<boolean> {
        const { messages, loadingOlder, exhausted } = this.store.state;
        const channel = this.channel;

        if (loadingOlder || exhausted || ! messages.length || ! channel) {
            return false;
        }

        this.store.set({ loadingOlder: true });

        try {
            const older = await this.hub.api.get<Message[]>(`/api/channels/${channel.id}/messages?before=${messages[0].id}`);

            if (this.channel !== channel) {
                return false;
            }

            const known = new Set(this.store.state.messages.map(message => message.id));

            this.store.set(state => ({
                exhausted: older.length === 0,
                messages: [...older.filter(message => ! known.has(message.id)), ...state.messages],
            }));

            return older.length > 0;
        } catch (failure) {
            this.hub.app.log('chat.older.error', { message: Failure.message(failure) });
            this.hub.app.toast(`não deu para carregar mensagens antigas: ${Failure.message(failure)}`, true);

            return false;
        } finally {
            if (this.channel === channel) {
                this.store.set({ loadingOlder: false });
            }
        }
    }

    append(message: Message): void {
        if (message.channel_id !== this.channel?.id) {
            return;
        }

        const messages = [...this.store.state.messages];
        const index = messages.findIndex(item => item.id === message.id);

        if (index !== -1) {
            messages[index] = message;
            this.store.set({ messages });

            return;
        }

        messages.push(message);

        if (messages.length > Chat.MAX_ROWS) {
            this.store.set({ messages: messages.slice(-Chat.MAX_ROWS), exhausted: false });

            return;
        }

        this.store.set({ messages });
    }

    remove(id: number): void {
        this.store.set(state => ({ messages: state.messages.filter(message => message.id !== id) }));
    }

    isMine(message: Message): boolean {
        return message.user.id === this.hub.user?.id;
    }

    canDelete(message: Message): boolean {
        return this.isMine(message) || Permissions.has(this.channel?.permissions ?? 0, Permissions.MANAGE_MESSAGES);
    }

    reply(message: Message | null): void {
        this.store.set({ replyTo: message });
    }

    markSeen(): void {
        if (this.store.state.newFrom !== null) {
            this.store.set({ newFrom: null });
        }
    }

    markUnreadFrom(id: number): void {
        if (this.store.state.newFrom === null) {
            this.store.set({ newFrom: id });
        }
    }

    async send(body: string): Promise<boolean> {
        const text = body.trim();
        const channel = this.channel;

        if (text === '' || ! channel) {
            return true;
        }

        const replyToId = this.store.state.replyTo?.id ?? null;

        const sent = await this.hub.attempt(async () => {
            this.append(await this.hub.api.post<Message>(`/api/channels/${channel.id}/messages`, { body: text, reply_to_id: replyToId }));

            return true;
        });

        if (sent) {
            this.reply(null);
            this.markSeen();
        }

        return Boolean(sent);
    }

    async edit(message: Message, body: string): Promise<boolean> {
        const text = body.trim();

        if (text === '' || text === message.body) {
            return true;
        }

        return Boolean(await this.hub.attempt(async () => {
            this.append(await this.hub.api.patch<Message>(`/api/messages/${message.id}`, { body: text }));

            return true;
        }));
    }

    async destroy(message: Message): Promise<void> {
        if (! await this.hub.app.confirm('Apagar esta mensagem?', 'Apagar')) {
            return;
        }

        const removed = await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/messages/${message.id}`);

            return true;
        });

        if (removed) {
            this.remove(message.id);
        }
    }
}
