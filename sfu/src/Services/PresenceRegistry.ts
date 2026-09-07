import type { WebSocket } from 'ws';

type Watcher = { socket: WebSocket; serverId: string };

type Membro = { name: string; avatar: string | null; joinedAt: number; sharing: boolean; screenProducerId: string | null; reconnecting: boolean };

type ChannelPresence = {
    members: ({ peerId: string } & Membro)[];
    startedAt: number;
};

/**
 * Who is in which voice channel, pushed over WebSocket to everyone on the
 * server — including those who joined no channel. Without this, we could only
 * know who was in the room by entering it.
 */
export class PresenceRegistry {
    private readonly watchers = new Map<string, Set<Watcher>>();

    private readonly channelServer = new Map<string, string>();

    private readonly channels = new Map<string, Map<string, Membro>>();

    link(channelId: string, serverId: string | undefined): void {
        if (serverId) {
            this.channelServer.set(channelId, serverId);
        }
    }

    watch(socket: WebSocket, serverId: string): Watcher {
        const watcher: Watcher = { socket, serverId };
        const set = this.watchers.get(serverId) ?? new Set<Watcher>();

        set.add(watcher);
        this.watchers.set(serverId, set);

        return watcher;
    }

    unwatch(socket: WebSocket, serverId: string): void {
        const set = this.watchers.get(serverId);

        if (!set) {
            return;
        }

        for (const watcher of set) {
            if (watcher.socket === socket) {
                set.delete(watcher);
            }
        }

        if (set.size === 0) {
            this.watchers.delete(serverId);
        }
    }

    enter(channelId: string, peerId: string, name: string, avatar: string | null): void {
        const members = this.channels.get(channelId) ?? new Map();

        // Preserve the join time on reconnection: the timer that others
        // see must not reset because signaling dropped.
        const joinedAt = members.get(peerId)?.joinedAt ?? Date.now();

        const previous = members.get(peerId);

        members.set(peerId, {
            name,
            avatar,
            joinedAt,
            sharing: previous?.sharing ?? false,
            screenProducerId: previous?.screenProducerId ?? null,
            reconnecting: false,
        });
        this.channels.set(channelId, members);
        this.publish(channelId);
    }

    /** Those outside the channel also need to see that someone is broadcasting. */
    setSharing(channelId: string, peerId: string, sharing: boolean, screenProducerId: string | null = null): void {
        const member = this.channels.get(channelId)?.get(peerId);

        if (! member || (member.sharing === sharing && member.screenProducerId === screenProducerId)) {
            return;
        }

        member.sharing = sharing;
        // The producer ID is included so someone who closed the broadcast can reopen it.
        member.screenProducerId = sharing ? screenProducerId : null;
        this.publish(channelId);
    }

    setReconnecting(channelId: string, peerId: string, reconnecting: boolean): void {
        const member = this.channels.get(channelId)?.get(peerId);

        if (! member || member.reconnecting === reconnecting) {
            return;
        }

        member.reconnecting = reconnecting;
        this.publish(channelId);
    }

    leave(channelId: string, peerId: string): void {
        const members = this.channels.get(channelId);

        if (!members) {
            return;
        }

        members.delete(peerId);

        if (members.size === 0) {
            this.channels.delete(channelId);
        }

        this.publish(channelId);
    }

    snapshot(serverId: string): Record<string, ChannelPresence> {
        const result: Record<string, ChannelPresence> = {};

        for (const [channelId, members] of this.channels) {
            if (this.channelServer.get(channelId) !== serverId) {
                continue;
            }

            const list = [...members].map(([peerId, member]) => ({ peerId, ...member }));

            result[channelId] = {
                members: list,
                // Those outside the channel also need to see how long the
                // conversation has been going: the local clock of someone who joined is not enough.
                startedAt: Math.min(...list.map(member => member.joinedAt)),
            };
        }

        return result;
    }

    private publish(channelId: string): void {
        const serverId = this.channelServer.get(channelId);

        if (!serverId) {
            return;
        }

        const payload = JSON.stringify({ event: 'presence', data: { channels: this.snapshot(serverId) } });

        for (const watcher of this.watchers.get(serverId) ?? []) {
            if (watcher.socket.readyState === watcher.socket.OPEN) {
                watcher.socket.send(payload);
            }
        }
    }
}
