import type { App } from './App.ts';
import { Failure } from './Failure.ts';
import type { Hub } from './Hub.ts';
import type { DirectConversation, DirectMessage, Person } from './Models.ts';
import { Store } from './Store.ts';

export type DirectState = {
    conversations: DirectConversation[];
    person: Person | null;
    messages: DirectMessage[];
    loading: boolean;
    failed: boolean;
};

export class Direct {
    readonly app: App;
    readonly hub: Hub;
    readonly store: Store<DirectState>;

    constructor(app: App, hub: Hub) {
        this.app = app;
        this.hub = hub;
        this.store = new Store<DirectState>({ conversations: [], person: null, messages: [], loading: false, failed: false });
    }

    unread(): number {
        return this.store.state.conversations.reduce((total, item) => total + item.unread, 0);
    }

    async loadConversations(): Promise<void> {
        if (! this.hub.user) {
            return;
        }

        const conversations = await this.hub.api.get<DirectConversation[]>('/api/dm').catch((failure: unknown) => {
            this.app.log('direct.load.error', { message: Failure.message(failure) });

            return null;
        });

        if (conversations) {
            this.store.set({ conversations });
        }
    }

    async open(person: Person): Promise<void> {
        this.store.set({ person, messages: [], loading: true, failed: false });

        const messages = await this.hub.attempt(() => this.hub.api.get<DirectMessage[]>(`/api/dm/${person.id}`));

        if (this.store.state.person?.id !== person.id) {
            return;
        }

        this.store.set(messages ? { messages, loading: false } : { loading: false, failed: true });
        this.clearUnread(person.id);
    }

    close(): void {
        this.store.set({ person: null, messages: [], failed: false });
    }

    forget(): void {
        this.store.set({ conversations: [], person: null, messages: [] });
    }

    send(body: string): Promise<boolean> {
        const person = this.store.state.person;

        return person ? this.sendTo(person, body) : Promise.resolve(false);
    }

    async sendTo(person: Person, body: string): Promise<boolean> {
        if (body.trim() === '') {
            return false;
        }

        const message = await this.hub.attempt(() => this.hub.api.post<DirectMessage>(`/api/dm/${person.id}`, { body }));

        if (! message) {
            return false;
        }

        this.receive(message, person);

        return true;
    }

    receive(message: DirectMessage, person: Person): void {
        const open = this.store.state.person?.id === person.id;

        this.store.set(state => ({
            messages: open && ! state.messages.some(item => item.id === message.id) ? [...state.messages, message] : state.messages,
            conversations: Direct.bump(state.conversations, person, message, open),
        }));

        if (open && ! message.mine) {
            this.markRead(person.id);
        }
    }

    static bump(conversations: DirectConversation[], person: Person, message: DirectMessage, read: boolean): DirectConversation[] {
        const last = { id: message.id, body: message.body, created_at: message.created_at, mine: message.mine };
        const current = conversations.find(item => item.user.id === person.id);
        const unread = read || message.mine ? 0 : (current?.unread ?? 0) + 1;
        const updated: DirectConversation = { user: current?.user ?? person, last, unread };

        return [updated, ...conversations.filter(item => item.user.id !== person.id)];
    }

    updateMessage(message: DirectMessage): void {
        this.store.set(state => ({
            messages: state.messages.map(item => (item.id === message.id ? message : item)),
            conversations: state.conversations.map(item => (item.last?.id === message.id ? { ...item, last: { ...item.last, body: message.body } } : item)),
        }));
    }

    removeMessage(id: number): void {
        this.store.set(state => ({
            messages: state.messages.filter(item => item.id !== id),
            conversations: state.conversations.map(item => (item.last?.id === id ? { ...item, last: { ...item.last, body: 'mensagem apagada' } } : item)),
        }));
    }

    clearUnread(userId: number): void {
        this.store.set(state => ({
            conversations: state.conversations.map(item => (item.user.id === userId ? { ...item, unread: 0 } : item)),
        }));
    }

    markRead(userId: number): void {
        this.clearUnread(userId);

        void this.hub.api.post(`/api/dm/${userId}/read`).catch((failure: unknown) => this.app.log('direct.read.error', { message: Failure.message(failure) }));
    }
}
