import type { App } from './App.ts';
import { Failure } from './Failure.ts';
import { Field } from './Field.ts';
import type { Hub } from './Hub.ts';
import type { Friendship, Person } from './Models.ts';
import { Store } from './Store.ts';

export type FriendsState = {
    list: Friendship[];
    loading: boolean;
    failed: boolean;
};

export class Friends {
    readonly app: App;
    readonly hub: Hub;
    readonly store: Store<FriendsState>;

    constructor(app: App, hub: Hub) {
        this.app = app;
        this.hub = hub;
        this.store = new Store<FriendsState>({ list: [], loading: false, failed: false });
    }

    other(friendship: Friendship): Person {
        return friendship.requester.id === this.hub.user?.id ? friendship.addressee : friendship.requester;
    }

    accepted(): Friendship[] {
        return this.store.state.list.filter(item => item.status === 'accepted');
    }

    incoming(): Friendship[] {
        return this.store.state.list.filter(item => item.status === 'pending' && item.addressee.id === this.hub.user?.id);
    }

    outgoing(): Friendship[] {
        return this.store.state.list.filter(item => item.status === 'pending' && item.requester.id === this.hub.user?.id);
    }

    blocked(): Friendship[] {
        return this.store.state.list.filter(item => item.status === 'blocked');
    }

    isFriend(userId: number): boolean {
        return this.accepted().some(item => this.other(item).id === userId);
    }

    async load(): Promise<void> {
        if (! this.hub.user) {
            return;
        }

        this.store.set({ loading: true });

        const list = await this.hub.api.get<Friendship[]>('/api/friends').catch((failure: unknown) => {
            this.app.log('friends.load.error', { message: Failure.message(failure) });

            return null;
        });

        this.store.set(list ? { list, loading: false, failed: false } : { loading: false, failed: true });
    }

    forget(): void {
        this.store.set({ list: [] });
    }

    update(friendship: Friendship): void {
        this.store.set(state => {
            const index = state.list.findIndex(item => item.id === friendship.id);

            if (index === -1) {
                return { list: [friendship, ...state.list] };
            }

            const list = [...state.list];

            list[index] = friendship;

            return { list };
        });
    }

    async request(email: string): Promise<boolean> {
        const friendship = await this.hub.attempt(() => {
            Field.requireEmail(email);

            return this.hub.api.post<Friendship>('/api/friends', { email: email.trim() });
        });

        if (! friendship) {
            return false;
        }

        this.update(friendship);
        this.app.toast('pedido de amizade enviado');

        return true;
    }

    async accept(friendship: Friendship): Promise<void> {
        const updated = await this.hub.attempt(() => this.hub.api.patch<Friendship>(`/api/friends/${friendship.id}`, { action: 'accept' }));

        if (updated) {
            this.update(updated);
        }
    }

    async block(friendship: Friendship): Promise<void> {
        const updated = await this.hub.attempt(() => this.hub.api.patch<Friendship>(`/api/friends/${friendship.id}`, { action: 'block' }));

        if (updated) {
            this.update(updated);
        }
    }

    async remove(friendship: Friendship): Promise<void> {
        const person = this.other(friendship);

        if (! await this.app.confirm(`Desfazer a amizade com ${person.name}?`, 'Desfazer')) {
            return;
        }

        const removed = await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/friends/${friendship.id}`);

            return true;
        });

        if (removed) {
            this.store.set(state => ({ list: state.list.filter(item => item.id !== friendship.id) }));
        }
    }
}
