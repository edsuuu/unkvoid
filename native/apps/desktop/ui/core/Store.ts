export type StorePatch<State> = Partial<State> | ((state: State) => Partial<State>);

export class Store<State extends object> {
    state: State;

    private readonly listeners = new Set<() => void>();

    readonly snapshot = (): State => this.state;

    readonly subscribe = (listener: () => void): (() => void) => {
        this.listeners.add(listener);

        return () => {
            this.listeners.delete(listener);
        };
    };

    constructor(state: State) {
        this.state = state;
    }

    set(patch: StorePatch<State>): void {
        const next = typeof patch === 'function' ? patch(this.state) : patch;

        this.replace({ ...this.state, ...next });
    }

    replace(state: State): void {
        this.state = state;

        for (const listener of this.listeners) {
            listener();
        }
    }
}
