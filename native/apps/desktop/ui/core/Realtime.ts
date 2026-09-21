import { SfuClient, type JoinResponse, type RoomIdentity, type SfuMessage } from './SfuClient.ts';

export type PresenceMember = { id: string; name: string };

export type ChannelListener = Record<string, (data: never) => void>;

type ChannelMessage = { event?: string; channel?: string; data?: unknown };

export class Realtime extends SfuClient {
    static readonly HERE = 'presence.here';

    static readonly IDENTIFIED: JoinResponse = {
        resumed: true,
        peerId: '',
        name: '',
        resumeKey: '',
        routerRtpCapabilities: {},
        peers: [],
        userId: '',
        can: [],
    };

    readonly listeners = new Map<string, Set<ChannelListener>>();
    readonly failed: (channel: string, failure: unknown) => void;

    constructor(failed: (channel: string, failure: unknown) => void) {
        super();
        this.failed = failed;
    }

    async setup(identity: RoomIdentity | null = null): Promise<JoinResponse> {
        await this.request('identify', identity ?? await this.resolveIdentity());
        await Promise.all([...this.listeners.keys()].map(channel =>
            this.join(channel).catch((failure: unknown) => this.failed(channel, failure))));

        return Realtime.IDENTIFIED;
    }

    async subscribe(channel: string, listener: ChannelListener): Promise<void> {
        const known = this.listeners.get(channel);

        if (known) {
            known.add(listener);

            return;
        }

        this.listeners.set(channel, new Set([listener]));

        try {
            await this.join(channel);
        } catch (failure) {
            this.unsubscribe(channel, listener);

            throw failure;
        }
    }

    async join(channel: string): Promise<void> {
        const { members } = await this.request<{ members: PresenceMember[] }>('subscribe', { channel });

        this.dispatch(channel, Realtime.HERE, { members });
    }

    unsubscribe(channel: string, listener: ChannelListener): void {
        const known = this.listeners.get(channel);

        if (! known?.delete(listener) || known.size > 0) {
            return;
        }

        this.listeners.delete(channel);
        void this.tolerate('unsubscribe', { channel });
    }

    dispatch(channel: string, event: string, data: unknown): void {
        for (const listener of [...this.listeners.get(channel) ?? []]) {
            listener[event]?.(data as never);
        }
    }

    handleMessage(message: SfuMessage): void {
        const { event, channel, data } = message as ChannelMessage;

        if (event !== undefined && channel !== undefined) {
            this.dispatch(channel, event, data);

            return;
        }

        super.handleMessage(message);
    }
}
