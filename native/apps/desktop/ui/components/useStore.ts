import { useSyncExternalStore } from 'react';

import type { Store } from '../core/Store.ts';

export function useStore<State extends object>(store: Store<State>): State {
    return useSyncExternalStore(store.subscribe, store.snapshot);
}
