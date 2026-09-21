import { Failure } from './Failure.ts';
import type { Hub } from './Hub.ts';
import { ImageShrinker } from './ImageShrinker.ts';
import type { Channel, Message } from './Models.ts';
import { Permissions } from './Permissions.ts';
import type { ChannelListener } from './Realtime.ts';
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
    images: PendingImage[];
    sending: boolean;
    unread: number;
};

export type PendingImage = {
    id: number;
    file: File;
    preview: string;
};

export class Chat {
    static readonly MAX_ROWS = 500;
    static readonly PAGE_SIZE = 50;
    static readonly MAX_IMAGES = 3;
    static readonly RENEW_EVERY_MS = 10 * 60_000;
    static readonly EMPTY: ChatState = { channel: null, messages: [], loading: false, loadingOlder: false, exhausted: false, failed: false, replyTo: null, newFrom: null, images: [], sending: false, unread: 0 };

    static mergeLatest<Item extends { id: number }>(known: Item[], latest: Item[]): Item[] {
        const start = latest[0]?.id ?? 0;
        const reachesKnown = latest.length === Chat.PAGE_SIZE && known.some(item => item.id >= start);

        return [...(reachesKnown ? known.filter(item => item.id < start) : []), ...latest];
    }

    readonly hub: Hub;
    channel: Channel | null = null;
    listener: ChannelListener | null = null;
    watched = true;
    imageSerial = 0;
    renewing = false;
    readonly renewedAt = new Map<number, number>();
    readonly store: Store<ChatState>;

    constructor(hub: Hub) {
        this.hub = hub;
        this.store = new Store<ChatState>(Chat.EMPTY);
    }

    async open(channel: Channel): Promise<void> {
        this.close();
        this.channel = channel;
        this.store.replace({ ...Chat.EMPTY, channel, loading: true });

        this.listener = {
            MessageSent: ({ message }: { message: Message }) => {
                if (message.user.id !== this.hub.user?.id) {
                    this.hub.app.sounds.message();

                    if (! this.watched) {
                        this.store.set(state => ({ unread: state.unread + 1 }));
                    }
                }

                this.append(message);
            },
            MessageUpdated: ({ message }: { message: Message }) => this.append(message),
            MessageDeleted: ({ id }: { id: number }) => this.remove(id),
        };

        let history: Message[] = [];

        try {
            await this.hub.realtime!.subscribe(`channel.${channel.id}`, this.listener);
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

    async catchUp(): Promise<void> {
        const channel = this.channel;

        if (! channel) {
            return;
        }

        const latest = await this.hub.api.get<Message[]>(`/api/channels/${channel.id}/messages`);

        if (this.channel === channel) {
            this.store.set(state => ({ messages: Chat.mergeLatest(state.messages, latest), exhausted: latest.length < Chat.PAGE_SIZE }));
        }
    }

    close(): void {
        if (this.channel && this.listener) {
            this.hub.realtime?.unsubscribe(`channel.${this.channel.id}`, this.listener);
        }

        for (const image of this.store.state.images) {
            URL.revokeObjectURL(image.preview);
        }

        this.channel = null;
        this.listener = null;
        this.renewedAt.clear();
        this.store.replace(Chat.EMPTY);
    }

    setWatched(watched: boolean): void {
        this.watched = watched;

        if (watched && this.store.state.unread > 0) {
            this.store.set({ unread: 0 });
        }
    }

    async attach(files: File[]): Promise<void> {
        const channel = this.channel;
        const room = Chat.MAX_IMAGES - this.store.state.images.length;

        if (! channel || files.length === 0) {
            return;
        }

        if (files.length > room) {
            this.hub.app.toast(`no máximo ${Chat.MAX_IMAGES} imagens por mensagem`, true);
        }

        for (const file of files.slice(0, Math.max(0, room))) {
            try {
                const fitted = await ImageShrinker.fit(file);

                if (this.channel !== channel || this.store.state.images.length >= Chat.MAX_IMAGES) {
                    return;
                }

                this.imageSerial += 1;
                this.store.set(state => ({ images: [...state.images, { id: this.imageSerial, file: fitted, preview: URL.createObjectURL(fitted) }] }));
            } catch (failure) {
                this.hub.app.log('chat.image.error', { name: file.name, type: file.type, size: file.size, message: Failure.message(failure) });
                this.hub.app.toast(Failure.message(failure), true);
            }
        }
    }

    detach(id: number): void {
        const image = this.store.state.images.find(item => item.id === id);

        if (! image) {
            return;
        }

        URL.revokeObjectURL(image.preview);
        this.store.set(state => ({ images: state.images.filter(item => item.id !== id) }));
    }

    async renewFiles(message: Message): Promise<void> {
        const channel = this.channel;
        const last = this.renewedAt.get(message.id);

        if (! channel || this.renewing || (last !== undefined && Date.now() - last < Chat.RENEW_EVERY_MS)) {
            return;
        }

        this.renewing = true;
        this.renewedAt.set(message.id, Date.now());

        try {
            const page = await this.hub.api.get<Message[]>(`/api/channels/${channel.id}/messages?before=${message.id + 1}`);

            if (this.channel !== channel) {
                return;
            }

            const fresh = new Map(page.map(item => [item.id, item]));

            for (const id of fresh.keys()) {
                this.renewedAt.set(id, Date.now());
            }

            this.store.set(state => ({ messages: state.messages.map(item => fresh.get(item.id) ?? item) }));
        } catch (failure) {
            this.hub.app.log('chat.files.renew.error', { message: Failure.message(failure) });
        } finally {
            this.renewing = false;
        }
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
        const { images, sending } = this.store.state;

        if ((text === '' && images.length === 0) || ! channel) {
            return true;
        }

        if (sending) {
            return false;
        }

        const replyToId = this.store.state.replyTo?.id ?? null;
        let payload: FormData | { body: string; reply_to_id: number | null } = { body: text, reply_to_id: replyToId };

        if (images.length > 0) {
            payload = new FormData();

            if (text !== '') {
                payload.append('body', text);
            }

            if (replyToId !== null) {
                payload.append('reply_to_id', String(replyToId));
            }

            for (const image of images) {
                payload.append('images[]', image.file);
            }
        }

        this.store.set({ sending: true });

        const sent = await this.hub.attempt(async () => {
            try {
                this.append(await this.hub.api.post<Message>(`/api/channels/${channel.id}/messages`, payload));
            } catch (failure) {
                this.hub.app.log('chat.send.error', { status: Failure.status(failure), images: images.length, message: Failure.message(failure) });

                throw Failure.status(failure) === 413
                    ? Object.assign(new Error('imagens grandes demais para uma mensagem só: mande menos, ou menores'), { status: 413 })
                    : failure;
            }

            return true;
        });

        if (this.channel !== channel) {
            return Boolean(sent);
        }

        this.store.set({ sending: false });

        if (sent) {
            for (const image of images) {
                this.detach(image.id);
            }

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
